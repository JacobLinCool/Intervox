#!/usr/bin/env bash
# Codesign the driver bundle with Developer ID + hardened runtime + secure
# timestamp (required for notarization). HAL plug-ins need no entitlements.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/driver_env.sh"

if [[ ! -d "$DRIVER_BUNDLE" ]]; then
    echo "Driver bundle not found; run scripts/build_driver.sh first." >&2
    exit 1
fi

codesign --force --options runtime --timestamp \
    --sign "$SIGN_IDENTITY" \
    "$DRIVER_BUNDLE"

echo "--- codesign --verify (strict, deep) ---"
codesign --verify --strict --deep --verbose=2 "$DRIVER_BUNDLE"

echo "--- codesign -dvvv ---"
codesign -dvvv "$DRIVER_BUNDLE" 2>&1 | grep -E \
    'Identifier|Authority|TeamIdentifier|Timestamp|Runtime|Signature'
