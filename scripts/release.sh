#!/bin/zsh
set -euo pipefail
cd "${0:A:h:h}"
# The zip a GitHub release carries: the app signed with a Developer ID and notarized by Apple. Errand's own update
# installs only an app that passes both, so it checks them here too. Keep the name: updates look for Errand-<version>.zip.
#   ERRAND_SIGN_IDENTITY   the certificate, as in: security find-identity -v -p codesigning
#   ERRAND_NOTARY_PROFILE  a keychain profile made once with: xcrun notarytool store-credentials
: "${ERRAND_SIGN_IDENTITY:?Set it to your Developer ID Application certificate}"
: "${ERRAND_NOTARY_PROFILE:?Set it to your notarytool keychain profile}"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
app='dist/Errand.app'
archive="dist/Errand-$version.zip"
zsh scripts/build.sh
rm -f "$archive"
/usr/bin/ditto -c -k --keepParent "$app" "$archive"
xcrun notarytool submit "$archive" --keychain-profile "$ERRAND_NOTARY_PROFILE" --wait
xcrun stapler staple "$app"
python3 scripts/build_privacy.py check "$app"
rm "$archive"
/usr/bin/ditto -c -k --keepParent "$app" "$archive"
/usr/bin/codesign --verify --deep --strict "$app"
/usr/sbin/spctl --assess --type execute "$app"
print "Ready for the v$version release: $archive"
