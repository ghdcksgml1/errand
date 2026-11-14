#!/bin/zsh
set -euo pipefail
cd "${0:A:h:h}"
# Native controls, actual Runtime persistence and restart, with temporary records and no account polling.
MACOSX_DEPLOYMENT_TARGET=13.0 cargo test --locked -p errand-app --features native-e2e --test onboarding_e2e
# Override with the signed release executable to check the exact distributable's native journal UI.
if [[ -z "${ERRAND_E2E_BINARY:-}" ]]; then
  MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --locked -p errand-app --bin errand
fi
binary="${ERRAND_E2E_BINARY:-$PWD/target/debug/errand}"
output="${ERRAND_E2E_OUTPUT:-$PWD/.build/e2e}"
mkdir -p "$output"
for language in ko en; do
  "$binary" --preview --preview-journal-drive --language "$language" "$output/journal-$language.png"
  for step in welcome accounts ai ready; do
    args=(--preview --preview-onboarding --language "$language")
    if [[ "$step" != welcome ]]; then args+=("--onboarding-$step"); fi
    "$binary" "${args[@]}" "$output/onboarding-$step-$language.png"
  done
done
print "Native E2E passed; screenshots in $output"
