#!/bin/zsh
set -euo pipefail
cd "${0:A:h:h}"
# Only the floating widget's AppKit checks; they show the dock and cards on screen for a moment.
MACOSX_DEPLOYMENT_TARGET=13.0 cargo test --locked -p errand-macos --test widget
