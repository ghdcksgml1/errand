#!/bin/zsh
set -euo pipefail
cd "${0:A:h:h}"
# Every crate's tests, the AppKit widget checks among them, against the versions Cargo.lock pins.
MACOSX_DEPLOYMENT_TARGET=13.0 cargo test --workspace --locked
# Icons are drawn from Lucide or the apps themselves; arrows, dingbats and emoji never stand in for them.
# Python rather than grep -P, which the grep that ships with macOS does not have.
if python3 - crates/*/src/**/*.rs <<'PY'
import re, sys
glyph = re.compile('[←-⇿⌀-⏿■-➿⬀-⯿\U0001F000-\U0001FAFF️]')
found = False
for path in sys.argv[1:]:
    for number, line in enumerate(open(path, encoding='utf-8'), 1):
        if glyph.search(line):
            print(f'{path}:{number}: {line.strip()}')
            found = True
sys.exit(0 if found else 1)
PY
then
  print -u2 'FAIL: the UI text above uses a symbol glyph; draw it with an icon instead'
  exit 1
fi
print 'No symbol glyphs in UI source'
python3 Tests/StartupTests.py
python3 Tests/InstallTests.py
python3 Tests/ConnectClaudeTests.py
python3 Tests/PackageTests.py
python3 Tests/BuildPrivacyTests.py
