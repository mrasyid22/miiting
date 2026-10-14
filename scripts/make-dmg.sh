#!/bin/zsh
# Package Miiting.app into a compressed, drag-to-Applications disk image.
# Usage: scripts/make-dmg.sh [path/to/Miiting.app]
#
# Environment:
#   VERSION   used in the file name (default: the app's CFBundleShortVersionString)
#   OUT_DIR   where to write the .dmg (default: the app's folder)
#   SIGN_ID   codesign identity for the .dmg itself; skipped when unset or "-"
set -euo pipefail
ROOT="${0:A:h:h}"
APP="${1:-$ROOT/build/Miiting.app}"
[[ -d "$APP" ]] || { echo "no app at $APP — run scripts/build-app.sh first"; exit 1; }
VERSION="${VERSION:-$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP/Contents/Info.plist")}"
OUT_DIR="${OUT_DIR:-${APP:h}}"
DMG="$OUT_DIR/Miiting-$VERSION.dmg"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
ditto "$APP" "$STAGE/Miiting.app"
ln -s /Applications "$STAGE/Applications"

rm -f "$DMG"
hdiutil create -volname "Miiting $VERSION" -srcfolder "$STAGE" -fs APFS -format ULFO -ov "$DMG" >/dev/null

if [[ -n "${SIGN_ID:-}" && "$SIGN_ID" != "-" ]]; then
  codesign --force --sign "$SIGN_ID" --timestamp "$DMG"
fi
hdiutil verify "$DMG" >/dev/null
echo "✓ $DMG ($(du -h "$DMG" | cut -f1))"
