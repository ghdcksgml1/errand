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

## 빌드와 테스트

macOS 13 이상, Command Line Tools, Rust 1.88 이상이 필요합니다.

```sh
zsh scripts/test.sh
zsh scripts/build.sh
python3 scripts/install.py --update
```

`scripts/test.sh`는 모든 테스트를 돌립니다. 위젯 검사는 실제 AppKit 창을 쓰므로 화면에 위젯이 잠깐 나타나고, GUI 세션이 필요합니다. 그래서 GitHub Actions에서는 위젯 검사를 빼고 돌립니다. 테스트는 임시 폴더에서 돌고 실제 로그인 정보를 쓰지 않습니다.

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
