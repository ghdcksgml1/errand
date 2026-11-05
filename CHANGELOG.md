# 변경 기록

새 버전을 낼 때마다 위에 더합니다. 버전을 올리는 법은 [개발 문서](docs/development.md)의 `버전 올리기`에 있습니다.

## 1.7.0

처음 공개한 버전입니다.

- Claude 사용량은 Claude Code가 알려주는 값을 씁니다. `새로고침`을 누르면 Claude Code의 `/usage`로 받아오고, 터미널에서 쓴다면 `python3 scripts/connect_claude.py`로 연결해 자동으로 받을 수 있습니다.
- Claude 카드에는 조회 시각 대신 `Claude Code 기준` 시각이 붙습니다.
- 모델별 주간 한도는 `/usage`로 받았을 때 보입니다.
- 새 버전이 나오면 위젯과 메뉴에 표시되고, 누르면 앱이 스스로 업데이트합니다.
- 공개하는 앱은 Developer ID로 서명하고 Apple 공증을 받습니다.
