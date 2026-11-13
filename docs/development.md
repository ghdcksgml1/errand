# 개발 문서

## 구조

Rust로 만든 Cargo 워크스페이스입니다. 코드는 `crates/` 아래에 있습니다.

| 크레이트 | 맡은 일 |
| --- | --- |
| `errand-core` | 사용량 해석, 작업 카드 상태, 화면에 보일 값과 문구. 운영체제와 상관없는 규칙만 있고 파일이나 네트워크를 건드리지 않습니다 |
| `errand-io` | 세션 기록 읽기, 기록 파일 저장, 바깥 서비스 호출. 호출은 모두 `api` 모듈의 `Api`를 거칩니다 |
| `errand-macos` | AppKit 화면과 네트워크, 앱 찾기, 업데이트 설치 같은 macOS 기능 |
| `errand-windows` | Windows 지원을 위한 자리입니다. 아직 비어 있습니다 |
| `errand-app` | 실행 파일. 위 크레이트를 이어 앱과 명령줄 모드를 실행합니다 |

앱은 한 방향으로 돕니다. 화면과 타이머가 `Msg`를 보내면 `errand-core`의 `update`가 상태를 바꾸고 `Effect`를 돌려주고, `errand-app`의 `Runtime`이 그것을 실행한 뒤 화면을 다시 그립니다.

작업방 서버는 별도 저장소(`errand-backend`)입니다. `ERRAND_ROOMS_API`로 다른 서버를 쓸 수 있고, `ERRAND_ROOMS_API=sample`이면 네트워크 없이 예시 데이터로 동작합니다.

작업방 인증정보는 Keychain의 `com.errand.rooms.credentials` 서비스에 보관하며, 계정 이름은 정규화한 서버 origin(스킴·호스트·포트)입니다. 다른 서버는 별도 인증정보를 사용하므로 공식 서버의 계정이나 비공개 방 권한을 공유하지 않습니다. 예전 설정의 `InstallID`는 공식 서버용으로만 이전하고 Keychain에서 읽기를 확인한 뒤 지웁니다. Keychain 오류가 나면 새 계정이나 평문 저장으로 우회하지 않고 다음 요청에서 재시도합니다.

HTTP 전송은 리다이렉트·캐시·쿠키를 사용하지 않고, 응답 헤더와 수신 청크에서 1MiB 제한을 검사합니다. 압축 응답도 해제된 크기를 기준으로 하며, API 계층에서도 JSON 파싱 전에 검사합니다. 첨부 파일은 열린 원본 파일의 `com.apple.quarantine`을 임시 복사본에 적용한 뒤 저장을 완료합니다. 보안 속성을 읽거나 쓰지 못하면 가져오기를 취소합니다.

## 빌드와 테스트

macOS 13 이상, Command Line Tools, Rust 1.88 이상이 필요합니다.

```sh
zsh scripts/test.sh
zsh scripts/build.sh
python3 scripts/install.py --update
```

`scripts/test.sh`는 모든 테스트를 돌립니다. 위젯 검사는 실제 AppKit 창을 쓰므로 화면에 위젯이 잠깐 나타나고, GUI 세션이 필요합니다. 그래서 GitHub Actions에서는 위젯 검사를 빼고 돌립니다. 테스트는 임시 폴더에서 돌고 실제 로그인 정보를 쓰지 않습니다.

실제 Keychain 이전 검사는 임시 키체인과 가짜 인증값만 사용하는 별도 테스트입니다. 개인 키체인은 변경하지 않으며, 실행 후 임시 키체인을 삭제합니다.

```sh
MACOSX_DEPLOYMENT_TARGET=13.0 cargo test --locked -p errand-macos --lib identity::tests::real_keychain_migration_and_origin_isolation -- --ignored --exact
```

## 미리보기

실제 계정 없이 화면을 PNG로 그립니다. 출력 경로는 맨 뒤에 둡니다.

```sh
"dist/Errand.app/Contents/MacOS/Errand" --preview --preview-tray --language ko "$PWD/.build/tray.png"
```

`--preview-floating`(위젯), `--preview-tray`(작업 카드), `--preview-simple`·`--preview-simple-open`(간단히 보기, 개수를 눌러 펼친 것), `--preview-badge`·`--preview-badge-open`(캐릭터만, 마우스를 올린 것), `--preview-details`(사용량 말풍선), `--preview-rooms`(방 목록), `--preview-room`(방을 펼친 위젯), `--preview-avatar`(프로필 꾸미기)가 있고, 방 목록 자리에 `--preview-create`(방 만들기), `--preview-invite`(초대 링크, `--preview-copied`와 함께), `--preview-members`(멤버 관리), `--preview-code`·`--preview-code-error`(코드 입력), `--preview-link`(링크로 열었을 때) 화면을 띄울 수 있으며, `--preview-errors`, `--preview-update` 같은 상태를 더할 수 있습니다. 작업 카드와 두 작은 위젯에는 `--preview-question`(질문), `--preview-stalled`(응답 없음), `--preview-pending`(체크한 직후), `--preview-calm`(부르는 일 없음), `--preview-no-tasks`(맡긴 일 없음), `--preview-low`(Claude 18%)를 더할 수 있습니다. README의 그림은 `zsh scripts/readme-images.sh`로 다시 그립니다.

## 버전 올리기

버전은 `Cargo.toml`의 `version` 한 곳에서 정합니다. `version`을 고치고, `cargo update --workspace`로 `Cargo.lock`을 맞추고, `CHANGELOG.md`에 적은 뒤, 그 커밋에 `v1.8.0` 같은 태그를 답니다.

## 출시하기

앱의 업데이트는 Developer ID로 서명하고 Apple 공증을 받은 앱만 설치합니다. 공증에 쓸 자격 증명은 처음 한 번 키체인에 저장합니다.

```sh
xcrun notarytool store-credentials errand-notary
```

`scripts/release.sh`가 Intel과 Apple Silicon을 함께 담아 서명하고 공증받은 `dist/Errand-<버전>.zip`을 만듭니다. 인증서 이름은 `security find-identity -v -p codesigning`에서 확인합니다.

```sh
ERRAND_SIGN_IDENTITY='Developer ID Application: 이름 (팀 ID)' ERRAND_NOTARY_PROFILE=errand-notary zsh scripts/release.sh
```

GitHub에서 `v<버전>` 태그로 Release를 만들고 이 파일을 이름 그대로 올립니다. 앱이 이 이름으로 파일을 찾습니다. 포크해서 다른 인증서로 서명한다면 `crates/errand-macos/src/updater.rs`의 `TEAM`과 `crates/errand-core/src/update.rs`의 저장소 주소를 바꿉니다.

## 아이콘

버튼과 메뉴의 아이콘은 [Lucide](https://lucide.dev)(ISC 라이선스)의 SVG를 벡터로 그립니다. 라이선스 전문은 `crates/errand-core/src/icons/data.rs`에 있습니다. 문자 기호나 이모지는 쓰지 않으며, `scripts/test.sh`가 화면 코드에 그런 문자가 있으면 실패합니다. 아이콘을 더하려면 `scripts/vendor_icons.py`의 `ICONS`에 이름을 넣고 실행합니다. Codex와 Claude 앱 아이콘은 복사하지 않고 설치된 앱에서 읽어옵니다.

## 첨부 작업

`errand-core::tasks`가 등록 초안, 제목·메모 생성 요청 번호, 개인 작업 상태를 관리합니다. 등록 버튼을 누른 뒤 저장이 성공한 경우에만 작업 목록을 바꿉니다. CLI 응답에는 초안과 요청 번호가 붙어서 취소, 재시도, 플랫폼·모델 변경 뒤의 오래된 응답을 무시합니다. 제목은 직접 수정하기 전까지만 자동으로 채우고, 메모는 `AcceptMemo` 전까지 제안으로만 보관합니다. 메모 칸에서 Tab 또는 추천 적용 버튼으로 채택하며, 사용자 입력은 항상 우선합니다.

`errand-io::tasks`는 원본을 복사하고 `tasks/tasks.json`을 원자적으로 교체합니다. 폴더는 0700, 첨부와 기록은 0600입니다. 취소된 첨부와 다음 실행에서 발견한 미등록 첨부를 정리하며, 기록을 읽지 못한 경우 빈 목록으로 덮어쓰지 않습니다.

`errand-io::task_settings`는 플랫폼과 플랫폼별 모델 ID를 `tasks/ai-settings.json`에 별도로 저장합니다. 이전 기록의 플랫폼을 기본값으로 이어받으며, 저장이 성공해야 실행 중 설정을 바꿉니다. Codex 목록은 해당 계정의 `models_cache.json`에서 표시 가능한 모델만 읽고, Claude Code는 모델 별칭을 제공합니다. 모델 ID 직접 입력과 빈 값(기본 모델)도 지원합니다. 선택한 ID는 두 CLI의 `--model` 인자로 전달하며 CLI 자체 설정은 수정하지 않습니다.

`errand-io::task_title`은 빈 임시 작업 폴더에서 로컬 CLI를 직접 실행합니다. Codex는 읽기 전용, 사용자 설정 제외, 셸/다중 에이전트/검색 비활성화로 실행합니다. Claude Code는 safe mode, 도구 없음, MCP 설정 격리, 훅 없음으로 저장된 Claude 로그인을 사용합니다. 각 CLI는 최대 75초, 이미지 변환은 장당 최대 12초 기다립니다. 취소 시 프로세스 그룹을 종료합니다. CLI가 지원하는 옵션은 설치 버전에 따라 다르므로 최신 CLI가 필요합니다.

`task_drop`은 세 위젯 크기에 공통으로 쓰는 AppKit 드롭 대상입니다. 안의 뷰는 기존 레이아웃과 동일한 크기를 유지하고, 드래그 중에는 캐릭터의 호버 확장을 멈춥니다. `task_form`은 입력 필드를 유지하면서 AI 상태와 첨부 목록만 갱신합니다. `settings`는 메뉴와 등록창에서 여는 설정 창입니다. 첨부 아이콘은 `assets/icons/attachment.svg`이며, 같은 경로를 `icons::attachment`와 `marks::draw_attachment`가 네이티브 벡터로 그립니다.

등록창은 `--preview --preview-task-form <PNG 경로>`, 첨부 상세는 `--preview-task-detail`, 설정 창은 `--preview-settings`로 확인합니다. 각 크기의 첨부 카드는 기존 미리보기 옵션에 `--preview-personal-task`를 더합니다. 실제 CLI 연동은 합성 샘플 파일로 다음과 같이 확인할 수 있습니다. 마지막 모델 ID는 생략할 수 있습니다. 이 명령도 해당 계정의 모델 사용량을 사용합니다.

```sh
cargo run -p errand-io --example task-title -- codex /path/to/codex /path/to/sample.png MODEL_ID
cargo run -p errand-io --example task-title -- claude /path/to/claude /path/to/sample.png sonnet
```
