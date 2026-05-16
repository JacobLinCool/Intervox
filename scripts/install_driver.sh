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

echo "--- verifying device is present ---"
if system_profiler SPAudioDataType 2>/dev/null | grep -q "Intervox"; then
    echo "OK: 'Intervox' input device is registered."
else
    echo "Device not visible yet. Check Console.app for '[Intervox]' logs and"
    echo "run: codesign -dvvv \"$INSTALLED_BUNDLE\"" >&2
fi
