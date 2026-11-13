# Codex 상태·이벤트 점검

2026-11-13 기준. 설치된 `codex-cli 0.167.0-alpha.4.1`의 생성 스키마, 로컬 기록의 구조, [공식 App Server 문서](https://learn.chatgpt.com/docs/app-server)를 대조했다. 테스트에는 실제 대화 대신 같은 구조의 가상 데이터를 쓴다.

## 원인과 수정

기존 `fold_codex`는 시작·완료·중단·사용자 메시지만 처리했다. `codex_row`도 `question: None`을 고정해, 질문 기록을 읽더라도 카드까지 전달할 수 없었다. 조회 SQL은 사용자 대화만 허용해 서브 에이전트를 제외했다.

| 기록 | 반영 방식 |
| --- | --- |
| `response_item/function_call`의 `request_user_input` | 질문을 표시하고 같은 `call_id`의 결과가 도착하면 해제한다. 취소·오류 결과도 대기를 끝낸다. |
| `request_user_input_async` | `questions[].title`을 표시한다. `{accepted: true}`는 질문 접수 확인이므로 대기를 유지한다. 다음 사용자 메시지 또는 턴 종료가 해제한다. |
| `event_msg/item_completed`의 `AgentMessage`, `delivery: async`, `questions` | 도구 호출이 없는 기록 구간에서도 비동기 질문을 복구한다. 이 메시지의 `phase: final_answer`만으로 턴을 완료하지 않는다. |
| `task_started`, `task_complete`, `turn_aborted` | 이전 질문을 지운다. 질문 중인 카드는 20분이 지나도 응답 없음으로 바뀌지 않는다. |
| `threads.source.subagent.thread_spawn.parent_thread_id` | 서브 에이전트를 부모 카드에 연결한다. 여러 단계의 하위 작업도 기존 카드 그룹화 규칙을 따른다. |
| guardian·review 등 내부 에이전트 | 부모를 명시한 실제 위임 작업만 포함한다. 내부 검토 작업을 사용자 카드로 만들지 않는다. |
| `thread_history_1.sqlite`의 `thread_turns`, `thread_items` | JSONL이 없거나 읽을 만한 턴이 없을 때 요청·답변·상태를 복원한다. 압축된 JSONL을 매번 해제하지 않는다. |

현재 파일이 있는 진행 중 대화는 JSONL을 우선한다. 동기 질문의 도구 호출과 아직 반환되지 않은 결과는 메시지 위주의 SQLite 기록만으로 복구되지 않을 수 있기 때문이다. SQLite는 읽기 전용 스냅샷으로 조회하며, 스키마를 읽을 수 없으면 기존 경로로 처리한다.

## 추가할 수 있는 상태

다음은 프로토콜에 존재하지만 이번 변경에 별도 상태로 추가하지 않은 항목이다. [공식 승인 흐름](https://learn.chatgpt.com/docs/app-server#approvals)과 [이벤트 목록](https://learn.chatgpt.com/docs/app-server#events)을 기준으로 구분했다.

| 후보 | 근거 이벤트·필드 | 필요한 작업 |
| --- | --- | --- |
| 실행·파일 변경·권한 승인 대기 | `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`, `item/permissions/requestApproval`, `serverRequest/resolved` | 실제로 그 대화를 실행하는 App Server의 요청·해제 이벤트에 연결해야 한다. `require_escalated`가 적힌 명령만으로 승인 대기라고 추정하면 자동 승인·거절을 오판한다. |
| 정확한 입력 대기·승인 대기 | `thread/status/changed`, `activeFlags: waitingOnUserInput / waitingOnApproval` | 같은 실행 서버의 실시간 상태가 필요하다. 별도로 실행한 서버는 데스크톱 앱의 메모리상 대기 상태를 대신하지 못한다. |
| 실행 실패 | `turn/completed`의 `failed`, `error`; 저장된 `thread_turns.status` | 사용자용 오류 상태·문구·집계 규칙이 필요하다. 이번 SQLite 읽기는 실패를 성공으로 표시하지 않고 `상태 확인 불가`로 둔다. |
| 서브 에이전트 기다리는 중 | `collabAgentToolCall`, `subAgentActivity`, 각 에이전트 상태 | 현재는 각 자식의 진행·완료·중단·질문 상태를 표시한다. 부모의 대기 이유를 따로 표시하려면 활동·대기 시작/종료를 추적해야 한다. |
| 명령 실행·파일 편집·검색·검토·컨텍스트 압축 | `commandExecution`, `fileChange`, `webSearch`, review, `contextCompaction` | 큰 상태를 늘리기보다 진행 중 카드의 보조 설명으로 표시할 수 있다. 종료 이벤트와 함께 관리해야 오래된 활동이 남지 않는다. |
| MCP 추가 입력·인증 | `mcpServer/elicitation/request`, OAuth·시작 상태 이벤트 | 도구별 입력 요청과 해제, 연결 실패를 구분해야 한다. 일반 도구 실행을 사용자 대기로 처리하지 않는다. |

대화 종류도 별도 점검 대상이다. 현재 사용자 대화·명시적 서브 에이전트만 조회하며, 로컬에서 확인한 `agent_created_thread`, `realtime_voice`, `voice_chat`은 포함하지 않는다. 이들을 추가하려면 일반 작업 카드로 집계할 범위와 각 기록의 생명주기를 먼저 정해야 한다.

## 검증 범위와 한계

- 동기 질문의 호출 ID 매칭, 비동기 접수·응답, 턴 재시작·완료·중단, 잘못된 도구 인자와 일반 메시지를 테스트한다.
- 부모·자식·손자 그룹화, 내부 에이전트 제외, 질문의 카드·헤더 표시를 테스트한다.
- JSONL이 없어지는 마이그레이션, SQLite WAL 갱신, 읽기 전용 조회, 알 수 없는 스키마, 실패를 성공으로 오인하지 않는 동작을 테스트한다.
- 비동기 질문에는 별도 답변 상관 ID가 없으므로 다음 사용자 메시지를 질문 해제로 취급한다. 여러 비동기 질문 요청이 겹치면 가장 최근 요청을 표시한다.
- 로컬 저장 형식은 공개된 안정 API가 아니다. Codex 업데이트 시 생성 스키마와 실제 저장 형식을 다시 확인해야 한다. 실행 중인 실제 사용자 대화를 중단하거나 질문을 삽입하는 검증은 하지 않았다.
