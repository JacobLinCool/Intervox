#!/usr/bin/env bash
# Notarize the signed driver bundle and staple the ticket. Requires a
# notarytool keychain profile (no secrets in this repo):
#   xcrun notarytool store-credentials "$NOTARY_PROFILE" \
#     --apple-id <id> --team-id "$TEAM_ID" --password <app-specific-password>
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/driver_env.sh"

if [[ ! -d "$DRIVER_BUNDLE" ]]; then
    echo "Driver bundle not found; build + sign first." >&2
    exit 1
fi

# Verify it is Developer-ID signed before wasting a notary round-trip.
codesign --verify --strict --deep --verbose=2 "$DRIVER_BUNDLE"

ZIP="$BUILD_DIR/Intervox.driver.zip"
rm -f "$ZIP"
/usr/bin/ditto -c -k --keepParent "$DRIVER_BUNDLE" "$ZIP"

echo "Submitting to Apple notary service (profile: $NOTARY_PROFILE)…"
xcrun notarytool submit "$ZIP" \
    --keychain-profile "$NOTARY_PROFILE" \
    --wait

echo "Stapling ticket…"
xcrun stapler staple "$DRIVER_BUNDLE"
xcrun stapler validate "$DRIVER_BUNDLE"
spctl -a -vvv -t install "$DRIVER_BUNDLE"
echo "Notarized + stapled: $DRIVER_BUNDLE"
