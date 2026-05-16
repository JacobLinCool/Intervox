#!/usr/bin/env bash
# Uninstall the Intervox HAL driver. Also restarts coreaudiod (interrupts all
# audio briefly). RUN THIS YOURSELF when no audio is in use.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/driver_env.sh"

if [[ ! -d "$INSTALLED_BUNDLE" ]]; then
    echo "Not installed: $INSTALLED_BUNDLE"
    exit 0
fi

if [[ "${INTERVOX_ASSUME_YES:-}" != "1" ]]; then
    read -r -p "Remove $INSTALLED_BUNDLE and restart coreaudiod? [y/N] " ans
    [[ "$ans" == "y" || "$ans" == "Y" ]] || { echo "Aborted."; exit 1; }
fi

sudo rm -rf "$INSTALLED_BUNDLE"
sudo killall coreaudiod || true
echo "Removed. coreaudiod restarted."
