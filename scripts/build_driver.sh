#!/usr/bin/env bash
# Build the Intervox HAL driver bundle (release).
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/driver_env.sh"

cmake -S "$DRIVER_DIR" -B "$BUILD_DIR" -DCMAKE_BUILD_TYPE=Release -G Ninja
cmake --build "$BUILD_DIR"

echo "Built: $DRIVER_BUNDLE"
file "$DRIVER_BUNDLE/Contents/MacOS/Intervox"
plutil -lint "$DRIVER_BUNDLE/Contents/Info.plist"
