#!/usr/bin/env bash
# Install the Intervox HAL driver. RUN THIS YOURSELF when no audio is in use.
#
#   ⚠️  This restarts coreaudiod (`killall coreaudiod`), which briefly
#       interrupts ALL audio on the machine — every app, including any
#       in-progress call or recording. Quit calls/DAWs first.
#
# Uses sudo for the privileged copy + service restart only.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/driver_env.sh"

if [[ ! -d "$DRIVER_BUNDLE" ]]; then
    echo "Driver bundle not found; run build + sign (+ notarize) first." >&2
    exit 1
fi

echo "About to install:"
echo "  src: $DRIVER_BUNDLE"
echo "  dst: $INSTALLED_BUNDLE"
echo "--- verifying source bundle trust chain ---"
codesign --verify --strict --deep --verbose=2 "$DRIVER_BUNDLE"
xcrun stapler validate "$DRIVER_BUNDLE"
spctl -a -vv -t install "$DRIVER_BUNDLE"

echo "This will run 'killall coreaudiod' and interrupt all audio briefly."
if [[ "${INTERVOX_ASSUME_YES:-}" != "1" ]]; then
    read -r -p "Continue? [y/N] " ans
    [[ "$ans" == "y" || "$ans" == "Y" ]] || { echo "Aborted."; exit 1; }
fi

sudo mkdir -p "$HAL_DIR"
sudo rm -rf "$INSTALLED_BUNDLE"
sudo cp -R "$DRIVER_BUNDLE" "$INSTALLED_BUNDLE"
sudo chown -R root:wheel "$INSTALLED_BUNDLE"
sudo chmod -R 755 "$INSTALLED_BUNDLE"

echo "Restarting coreaudiod…"
sudo killall coreaudiod || true
sleep 2

echo "--- verifying installed bundle ---"
codesign --verify --strict --deep --verbose=2 "$INSTALLED_BUNDLE"
xcrun stapler validate "$INSTALLED_BUNDLE"
spctl -a -vv -t install "$INSTALLED_BUNDLE"
echo "Installed. Device enumeration is intentionally not run here; use the app's"
echo "bounded audio-device refresh so a wedged CoreAudio query cannot hang setup."
