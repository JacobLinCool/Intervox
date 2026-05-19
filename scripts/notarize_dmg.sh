#!/usr/bin/env bash
# Notarize a signed Intervox DMG and staple the ticket. Requires a notarytool
# keychain profile (no secrets in this repo):
#   xcrun notarytool store-credentials "$NOTARY_PROFILE" \
#     --apple-id <id> --team-id "$TEAM_ID" --password <app-specific-password>
#
# Usage: scripts/notarize_dmg.sh <path-to-dmg>
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/driver_env.sh"

DMG_PATH="${1:?Usage: scripts/notarize_dmg.sh <path-to-dmg>}"

if [[ ! -f "$DMG_PATH" ]]; then
    echo "DMG not found: $DMG_PATH" >&2
    exit 1
fi

# Verify it is Developer-ID signed before wasting a notary round-trip.
codesign --verify --strict --verbose=2 "$DMG_PATH"

echo "Submitting $(basename "$DMG_PATH") to Apple notary service (profile: $NOTARY_PROFILE)…"
xcrun notarytool submit "$DMG_PATH" \
    --keychain-profile "$NOTARY_PROFILE" \
    --wait

echo "Stapling DMG ticket…"
xcrun stapler staple "$DMG_PATH"
xcrun stapler validate "$DMG_PATH"
spctl -a -vvv -t open --context context:primary-signature "$DMG_PATH"
echo "Notarized + stapled: $DMG_PATH"
