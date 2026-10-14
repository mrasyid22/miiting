#!/bin/zsh
# Notarize and staple a signed .dmg with an App Store Connect API key.
# Usage: scripts/notarize.sh path/to/Miiting-x.y.z.dmg
#
# Environment (all required):
#   APPLE_API_KEY_PATH   path to the AuthKey_XXXXXX.p8 file
#   APPLE_API_KEY_ID     the key ID
#   APPLE_API_ISSUER_ID  the issuer ID (App Store Connect › Users and Access › Integrations)
set -euo pipefail
DMG="$1"
: "${APPLE_API_KEY_PATH:?}" "${APPLE_API_KEY_ID:?}" "${APPLE_API_ISSUER_ID:?}"

echo "› submitting $DMG for notarization (usually a few minutes)"
xcrun notarytool submit "$DMG" \
  --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID" \
  --wait --timeout 30m
xcrun stapler staple "$DMG"
spctl --assess --type open --context context:primary-signature --verbose "$DMG"
echo "✓ notarized $DMG"
