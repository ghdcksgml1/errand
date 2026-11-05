#!/bin/zsh
set -euo pipefail
cd "${0:A:h:h}"
# The README's pictures, drawn by the built app from its preview data, so nothing of this Mac's work or accounts
# shows in them. Only the Codex and Claude icons come from here, from the apps installed.
errand='dist/Errand.app/Contents/MacOS/Errand'
images="$PWD/docs/images"
mkdir -p "$images"
preview() {
  "$errand" --preview --language ko "$@" > /dev/null
}
preview --preview-tray "$images/tasks.png"
preview --preview-details "$images/usage.png"
preview --preview-room "$images/room.png"
preview --preview-avatar "$images/character-editor.png"
# One for each body, and each introduces a part of the app.
characters=(
  tasks 'avatar:triangle,sprout,happy,grin,hoodie,blush,6366F1'
  usage 'avatar:dome,bear,sparkly,smile,overalls,blush,F97316'
  open 'avatar:square,cat,dots,cat,tee,plain,14B8A6'
  rooms 'avatar:cloud,beanie,sleepy,round,stripes,blush,EC4899'
  private 'avatar:hexagon,sprout,squint,flat,collar,plain,3B82F6'
)
for name code in $characters; do
  preview --preview-character "$code" "$images/character-$name.png"
done
print "Drew the README's pictures in docs/images"
