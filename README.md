<p align="center">
  <img src="docs/images/character-tasks.png" width="104" alt="">
  <img src="docs/images/character-usage.png" width="104" alt="">
  <img src="docs/images/character-open.png" width="104" alt="">
  <img src="docs/images/character-rooms.png" width="104" alt="">
  <img src="docs/images/character-private.png" width="104" alt="">
</p>

<h1 align="center">Errand</h1>

<p align="center"><b>Codex와 Claude에 맡겨둔 심부름을 챙겨주는 작은 맥 앱</b></p>

<p align="center">
AI에게 일을 맡기고 다른 일을 하다 보면, 뭘 시켰는지 잊고 사용량이 바닥난 것도 뒤늦게 알게 됩니다.<br>
Errand는 시킨 일과 남은 사용량을 바탕화면 한쪽에 계속 띄워 둡니다.
</p>

<p align="center"><img src="docs/images/tasks.png" width="336" alt="작업 카드 세 장이 사용량 위젯 위에 쌓여 있는 Errand 화면"></p>

## 심부름꾼들을 소개할게요

<table>
  <tr>
    <td width="112" align="center"><img src="docs/images/character-tasks.png" width="96" alt=""></td>
    <td><b>"시킨 일은 제가 기억할게요."</b><br>Codex와 Claude에 보낸 요청이 카드로 쌓입니다. 직접 체크하기 전까지는 사라지지 않아서, 맡겨둔 일을 놓치지 않아요.</td>
  </tr>
  <tr>
    <td align="center"><img src="docs/images/character-usage.png" width="96" alt=""></td>
    <td><b>"남은 양은 제가 지켜볼게요."</b><br>두 서비스의 남은 사용량을 배터리처럼 보여주고, 언제 초기화되는지도 알려줍니다.</td>
  </tr>
  <tr>
    <td align="center"><img src="docs/images/character-open.png" width="96" alt=""></td>
    <td><b>"결과가 나오면 바로 데려다드려요."</b><br>카드를 누르면 그 대화가 Codex나 Claude에서 바로 열립니다.</td>
  </tr>
  <tr>
    <td align="center"><img src="docs/images/character-rooms.png" width="96" alt=""></td>
    <td><b>"혼자 일해도 같이 일하는 기분으로요."</b><br>작업방에 들어가면 다른 사람들이 몇 개의 일을 돌리고 있는지만 보이고, 내 캐릭터가 함께 앉아 있습니다.</td>
  </tr>
  <tr>
    <td align="center"><img src="docs/images/character-private.png" width="96" alt=""></td>
    <td><b>"무슨 일인지는 아무에게도 말 안 해요."</b><br>작업방에 작업 내용은 보내지 않습니다. 작업방에 전하는 것은 AI 작업 개수와 캐릭터, 비공개 방에서의 닉네임뿐입니다. 첨부 작업의 제목을 만들 때는 선택한 로컬 CLI가 첨부 내용을 해당 모델 서비스로 전송합니다. 로그인 토큰은 복사하거나 바꾸지 않습니다.</td>
  </tr>
</table>

## 화면

<table>
  <tr>
    <td align="center"><img src="docs/images/usage.png" width="420" alt="Codex와 Claude의 한도별 사용량과 초기화 시간을 보여주는 말풍선"><br>한도별 사용량과 다음 초기화</td>
  </tr>
  <tr>
    <td align="center"><img src="docs/images/room.png" width="304" alt="작업방 탭 아래에 사람들의 캐릭터와 작업 개수가 보이는 위젯"><br>작업방: 캐릭터와 별명, 개수, 들어온 시간만 보입니다</td>
  </tr>
  <tr>
    <td align="center"><img src="docs/images/character-editor.png" width="312" alt="몸, 머리 위, 눈, 입, 옷, 볼을 고르는 캐릭터 편집 말풍선"><br>내 캐릭터는 직접 꾸밀 수 있습니다</td>
  </tr>
</table>

앱이 실제로 그린 화면이고, 내용은 예시 데이터입니다.

## 시작하기

[Releases](https://github.com/ghdcksgml1/errand/releases/latest)에서 `Errand-<버전>.zip`을 받아 풀고, `Errand.app`을 `~/Applications`로 옮겨서 엽니다. macOS 13 이상에서 동작합니다.

로그인을 따로 받지 않고, 이 Mac에 이미 로그인된 계정을 그대로 씁니다.

- **Codex**: Codex(ChatGPT) 데스크톱 앱에 로그인돼 있으면 바로 보입니다.
- **Claude**: Claude 데스크톱 앱이 있으면 작업 카드는 바로 보이고, 사용량은 앱을 켤 때와 2분마다 받아옵니다. `새로고침`을 누르면 바로 다시 받아옵니다. 터미널에서 Claude Code를 쓴다면 `python3 scripts/connect_claude.py`로 status line도 연결할 수 있습니다.

새 버전이 나오면 위젯의 프로필 옆에 표시됩니다. 누르면 내려받아 서명을 확인한 뒤 앱을 바꾸고 다시 시작합니다. 무엇이 바뀌었는지는 [CHANGELOG](CHANGELOG.md)에 있습니다.

### 직접 빌드하기

Command Line Tools(`xcode-select --install`)와 [Rust](https://rustup.rs) 1.88 이상이 필요합니다.

```sh
git clone https://github.com/ghdcksgml1/errand.git
cd errand
zsh scripts/build.sh
python3 scripts/install.py --autostart
open ~/Applications/Errand.app
```

새 버전은 `git pull` 뒤에 `zsh scripts/build.sh`와 `python3 scripts/install.py --update`로 올립니다.

## 더 알아보기

- [사용 설명서](docs/guide.md): 위젯, 작업 카드, 사용량, 작업방, 저장되는 내용
- [개발 문서](docs/development.md): 구조, 빌드와 테스트, 출시

[MIT 라이선스](LICENSE)로 공개합니다. 아이콘은 [Lucide](https://lucide.dev)를 씁니다.

Errand는 개인이 만든 비공식 앱이며 OpenAI, Anthropic과 관계가 없습니다.

## 파일과 이미지로 작업 등록

메신저 캡처나 파일을 펫 위에 놓으면 등록창이 열립니다. 크게 보기, 간단히 보기, 캐릭터만 보기에서 모두 사용할 수 있습니다. 여러 파일을 한 번에 놓으면 한 작업에 묶입니다.

첨부 미리보기와 제목을 확인하고 메모를 적은 뒤 **작업 등록**을 누릅니다. 등록 전에는 목록에 추가되지 않으며, 취소하면 임시 첨부를 지웁니다. 등록한 카드를 누르면 첨부를 다시 열 수 있고, 기존 카드처럼 완료 체크와 되돌리기를 사용할 수 있습니다.

제목은 로그인된 **Codex 또는 Claude Code CLI**로 제안합니다. 설치된 데스크톱 앱의 CLI와 일반 설치 경로를 찾으며, CLI가 없거나 제목 생성에 실패해도 직접 제목을 입력할 수 있습니다. 직접 수정한 제목은 늦게 도착한 AI 응답으로 바뀌지 않습니다. CLI는 컴퓨터에서 실행되지만 추론은 해당 모델 서비스가 처리하며, 기존 계정의 사용량이 발생할 수 있습니다.

파일은 한 작업에 최대 12개, 각각 50MB까지 첨부할 수 있습니다. 이미지 제목 생성에는 앞의 이미지 최대 4장을 축소해 사용합니다. 텍스트 파일은 앞부분을, PDF 등 읽을 수 없는 파일은 파일명을 참고합니다. 첨부는 `~/Library/Application Support/Errand/tasks`에 별도로 복사하므로 원본을 옮겨도 유지되며, 개인 첨부 작업의 내용과 개수는 작업방에 보내지 않습니다.
