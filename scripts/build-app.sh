#!/bin/zsh
# Build Miiting.app into build/. Usage: scripts/build-app.sh [--skip-core]
#
# Environment:
#   BUILD_DIR      output folder (default: build/)
#   VERSION        CFBundleShortVersionString to stamp, e.g. 0.2.0 (default: Info.plist)
#   BUILD_NUMBER   CFBundleVersion to stamp, e.g. the CI run number (default: Info.plist)
#   SIGN_ID        codesign identity; "-" (default) signs ad hoc for local use
set -euo pipefail
ROOT="${0:A:h:h}"
APP_SRC="$ROOT/apps/macos"
OUT="${BUILD_DIR:-$ROOT/build}"
BUNDLE="$OUT/Miiting.app"
CONFIG="${CONFIG:-release}"

if [[ "${1:-}" != "--skip-core" ]]; then
  "$ROOT/scripts/build-core.sh"
fi

echo "› swift build ($CONFIG)"
swift build --package-path "$APP_SRC" -c "$CONFIG"
BIN="$(swift build --package-path "$APP_SRC" -c "$CONFIG" --show-bin-path)/Miiting"

echo "› bundling"
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp "$BIN" "$BUNDLE/Contents/MacOS/Miiting"
cp "$APP_SRC/Resources/Info.plist" "$BUNDLE/Contents/Info.plist"
[[ -f "$APP_SRC/Resources/AppIcon.icns" ]] && cp "$APP_SRC/Resources/AppIcon.icns" "$BUNDLE/Contents/Resources/AppIcon.icns"
# Localizations (Localizable.strings, InfoPlist.strings) live in *.lproj folders.
for lproj in "$APP_SRC"/Resources/*.lproj; do
  cp -R "$lproj" "$BUNDLE/Contents/Resources/"
done

PLIST="$BUNDLE/Contents/Info.plist"
[[ -n "${VERSION:-}" ]] && /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$PLIST"
[[ -n "${BUILD_NUMBER:-}" ]] && /usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD_NUMBER" "$PLIST"

# Ad-hoc signature for local use. Releases are signed with a Developer ID, using the
# hardened runtime that notarization requires (see docs/releasing.md).
SIGN_ID="${SIGN_ID:--}"
if [[ "$SIGN_ID" == "-" ]]; then
  codesign --force --sign - --entitlements "$APP_SRC/Resources/Miiting.entitlements" "$BUNDLE"
else
  codesign --force --sign "$SIGN_ID" --options runtime --timestamp \
    --entitlements "$APP_SRC/Resources/Miiting.entitlements" "$BUNDLE"
fi
echo "✓ $BUNDLE"
