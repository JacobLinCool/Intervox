#!/usr/bin/env bash
# Build a signed, distributable Intervox.dmg from an already-built and signed
# (for release: notarized + stapled) Intervox.app.
#
# The DMG contains Intervox.app plus an /Applications symlink for drag-install.
# The HAL driver is installed later, at first run, by the app's own privileged
# helper, so no .pkg/installer is needed here.
#
# Usage: scripts/make_dmg.sh [path/to/Intervox.app]
#
# Environment:
#   SIGN_IDENTITY                 Developer ID Application identity.
#   INTERVOX_DMG_FINDER_LAYOUT=1  Prettify the Finder window via AppleScript.
#                                 Off by default: Finder scripting can hang in
#                                 headless/SSH/CI sessions.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/driver_env.sh"

APP_BUNDLE="${1:-$REPO_ROOT/src-tauri/target/release/bundle/macos/Intervox.app}"

if [[ ! -d "$APP_BUNDLE" ]]; then
    echo "App bundle not found: $APP_BUNDLE" >&2
    exit 1
fi

# tauri.conf.json is JSON; plutil reads JSON and is guaranteed on macOS.
VERSION="$(/usr/bin/plutil -extract version raw -o - \
    "$REPO_ROOT/src-tauri/tauri.conf.json")"
VOL_NAME="Intervox"
DMG_DIR="$REPO_ROOT/src-tauri/target/release/bundle/dmg"
DMG_PATH="$DMG_DIR/Intervox-${VERSION}.dmg"

mkdir -p "$DMG_DIR"
rm -f "$DMG_PATH"

STAGE="$(mktemp -d)"
RW_DMG="$(mktemp -u).dmg"
MOUNT_DIR=""
cleanup() {
    if [[ -n "$MOUNT_DIR" && -d "$MOUNT_DIR" ]]; then
        hdiutil detach "$MOUNT_DIR" -quiet 2>/dev/null || true
        rmdir "$MOUNT_DIR" 2>/dev/null || true
    fi
    rm -rf "$STAGE"
    rm -f "$RW_DMG"
}
trap cleanup EXIT

echo "Staging app into DMG layout…"
# ditto preserves symlinks, resource forks, and code-signature metadata.
ditto "$APP_BUNDLE" "$STAGE/Intervox.app"
ln -s /Applications "$STAGE/Applications"

# Best-effort volume icon from the app icon (needs Xcode CLT's SetFile).
ICON_SRC="$REPO_ROOT/src-tauri/icons/icon.icns"
HAVE_VOL_ICON=0
if [[ -f "$ICON_SRC" ]] && command -v SetFile >/dev/null 2>&1; then
    cp "$ICON_SRC" "$STAGE/.VolumeIcon.icns"
    HAVE_VOL_ICON=1
fi

# Size the read/write image with headroom over the staged payload.
SIZE_KB="$(du -sk "$STAGE" | awk '{print $1}')"
SIZE_MB=$(( SIZE_KB / 1024 + 64 ))

echo "Creating read/write image (${SIZE_MB} MB)…"
hdiutil create -srcfolder "$STAGE" -volname "$VOL_NAME" \
    -fs HFS+ -format UDRW -size "${SIZE_MB}m" "$RW_DMG" -quiet

MOUNT_DIR="$(mktemp -d)"
hdiutil attach "$RW_DMG" -mountpoint "$MOUNT_DIR" \
    -nobrowse -noautoopen -quiet

if [[ "$HAVE_VOL_ICON" -eq 1 && -f "$MOUNT_DIR/.VolumeIcon.icns" ]]; then
    SetFile -a C "$MOUNT_DIR" || true
fi

# Optional prettified Finder window. Disabled by default because Finder
# AppleScript can block indefinitely when no window server is attached.
if [[ "${INTERVOX_DMG_FINDER_LAYOUT:-0}" == "1" ]]; then
    osascript - "$VOL_NAME" <<'APPLESCRIPT' || true
on run argv
  set volName to item 1 of argv
  tell application "Finder"
    tell disk volName
      open
      set current view of container window to icon view
      set toolbar visible of container window to false
      set statusbar visible of container window to false
      set the bounds of container window to {200, 150, 800, 480}
      set vopts to the icon view options of container window
      set arrangement of vopts to not arranged
      set icon size of vopts to 96
      set position of item "Intervox.app" of container window to {160, 180}
      set position of item "Applications" of container window to {440, 180}
      update without registering applications
      delay 1
      close
    end tell
  end tell
end run
APPLESCRIPT
fi

sync
hdiutil detach "$MOUNT_DIR" -quiet
rmdir "$MOUNT_DIR" 2>/dev/null || true
MOUNT_DIR=""

echo "Converting to compressed read-only image…"
hdiutil convert "$RW_DMG" -format UDZO -imagekey zlib-level=9 \
    -o "$DMG_PATH" -quiet

echo "Signing DMG with ${SIGN_IDENTITY}…"
codesign --force --sign "$SIGN_IDENTITY" --timestamp "$DMG_PATH"
codesign --verify --strict --verbose=2 "$DMG_PATH"

echo "Built DMG: $DMG_PATH"
