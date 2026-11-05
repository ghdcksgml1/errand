#!/bin/zsh
set -euo pipefail
cd "${0:A:h:h}"
mkdir -p .build dist
# Cargo.lock pins every dependency; --locked stops the build rather than quietly resolving newer ones.
MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release --locked --bin errand --bin make-icon

app='dist/Errand.app'
# Builds before 1.5 also made a Codex A launcher; the app replaces it.
rm -rf "$app" 'dist/Codex A.app'
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
if [[ -n "${ERRAND_SIGN_IDENTITY:-}" ]]; then
  # A release also has to run on Intel Macs; everyday builds stay native, and quicker.
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release --locked --bin errand --target "$target"
  done
  /usr/bin/lipo -create target/{aarch64,x86_64}-apple-darwin/release/errand -output "$app/Contents/MacOS/Errand"
else
  cp target/release/errand "$app/Contents/MacOS/Errand"
fi
iconset='.build/icon.iconset'
rm -rf "$iconset"
mkdir -p "$iconset"
target/release/make-icon .build/icon.png
# Each size is drawn rather than scaled down, so the smallest ones can drop detail that would blur.
for size in 16 32 128 256 512; do
  target/release/make-icon "$iconset/icon_${size}x${size}.png" "$size"
  target/release/make-icon "$iconset/icon_${size}x${size}@2x.png" $((size * 2))
done
/usr/bin/iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
python3 - "$app" <<'PY'
import plistlib, re, sys
from pathlib import Path
app = Path(sys.argv[1])
# The workspace's version, the one the binary was built with.
manifest = Path("Cargo.toml").read_text()
version = re.search(r'^\[workspace\.package\]$.*?^version = "([^"]+)"', manifest, re.M | re.S).group(1)
info = {
    'CFBundleName': 'Errand', 'CFBundleDisplayName': 'Errand', 'CFBundleIdentifier': 'com.errand.app',
    'CFBundleExecutable': 'Errand', 'CFBundlePackageType': 'APPL',
    'CFBundleShortVersionString': version, 'CFBundleVersion': version,
    'LSMinimumSystemVersion': '13.0', 'NSHighResolutionCapable': True,
    'NSPrincipalClass': 'NSApplication',
    'CFBundleIconFile': 'AppIcon',
    # The app words itself in these; declaring them lists Errand under System Settings' per-app languages.
    'CFBundleDevelopmentRegion': 'en',
    'CFBundleLocalizations': ['en', 'ko'],
}
with (app / 'Contents/Info.plist').open('wb') as f:
    plistlib.dump(info, f)
PY
# Signed for this computer, unless ERRAND_SIGN_IDENTITY names a Developer ID certificate, as scripts/release.sh does.
if [[ -n "${ERRAND_SIGN_IDENTITY:-}" ]]; then
  /usr/bin/codesign --force --options runtime --timestamp --sign "$ERRAND_SIGN_IDENTITY" "$app"
else
  /usr/bin/codesign --force --sign - "$app"
fi
print 'Built Errand.app in dist/'
