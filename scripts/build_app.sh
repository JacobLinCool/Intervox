#!/usr/bin/env bash
# Build the packaged Intervox macOS app bundle.
#
# Default mode builds and signs the HAL driver, then runs `pnpm tauri build`.
# Use `--release` when producing a distributable build: it runs the automated
# checks, notarizes/staples the driver before packaging, then notarizes/staples
# the final Intervox.app bundle.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/driver_env.sh"

RUN_CHECKS=0
NOTARIZE_DRIVER=0
NOTARIZE_APP=0

usage() {
    cat <<'EOF'
Usage: scripts/build_app.sh [options]

Options:
  --checks            Run cargo/pnpm checks before building.
  --notarize-driver  Notarize and staple driver/build/Intervox.driver before packaging.
  --notarize-app     Notarize and staple the built Intervox.app.
                     This also notarizes the driver first.
  --release          Equivalent to --checks --notarize-driver --notarize-app.
  -h, --help         Show this help.

Environment:
  SIGN_IDENTITY      Developer ID Application signing identity.
  NOTARY_PROFILE    notarytool credentials profile. Default: intervox-notary.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --checks)
            RUN_CHECKS=1
            ;;
        --notarize-driver)
            NOTARIZE_DRIVER=1
            ;;
        --notarize-app)
            NOTARIZE_DRIVER=1
            NOTARIZE_APP=1
            ;;
        --release)
            RUN_CHECKS=1
            NOTARIZE_DRIVER=1
            NOTARIZE_APP=1
            ;;
        --)
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Required command not found: $1" >&2
        exit 1
    fi
}

run() {
    printf '\n==>'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

need_cmd cargo
need_cmd cmake
need_cmd codesign
need_cmd ninja
need_cmd plutil
need_cmd pnpm

if [[ "$NOTARIZE_DRIVER" -eq 1 || "$NOTARIZE_APP" -eq 1 ]]; then
    need_cmd spctl
    need_cmd xcrun
fi

APP_BUNDLE="$REPO_ROOT/src-tauri/target/release/bundle/macos/Intervox.app"
APP_ZIP="$REPO_ROOT/src-tauri/target/release/bundle/macos/Intervox.app.zip"
BUNDLED_DRIVER="$APP_BUNDLE/Contents/Resources/driver/build/Intervox.driver"

cd "$REPO_ROOT"

if [[ "$RUN_CHECKS" -eq 1 ]]; then
    run cargo test --workspace
    run cargo test --manifest-path src-tauri/Cargo.toml
    run pnpm test
    run pnpm check
fi

run "$REPO_ROOT/scripts/build_driver.sh"
run "$REPO_ROOT/scripts/sign_driver.sh"

if [[ "$NOTARIZE_DRIVER" -eq 1 ]]; then
    run "$REPO_ROOT/scripts/notarize_driver.sh"
fi

run pnpm tauri build

if [[ ! -d "$APP_BUNDLE" ]]; then
    echo "Expected app bundle not found: $APP_BUNDLE" >&2
    exit 1
fi

if [[ ! -d "$BUNDLED_DRIVER" ]]; then
    echo "Packaged driver resource not found: $BUNDLED_DRIVER" >&2
    exit 1
fi

echo
echo "--- app Info.plist ---"
plutil -lint "$APP_BUNDLE/Contents/Info.plist"

echo
echo "--- app codesign --verify (strict, deep) ---"
codesign --verify --strict --deep --verbose=2 "$APP_BUNDLE"

echo
echo "--- bundled driver codesign --verify (strict, deep) ---"
codesign --verify --strict --deep --verbose=2 "$BUNDLED_DRIVER"

if [[ "$NOTARIZE_APP" -eq 1 ]]; then
    rm -f "$APP_ZIP"
    run /usr/bin/ditto -c -k --keepParent "$APP_BUNDLE" "$APP_ZIP"

    echo
    echo "Submitting Intervox.app to Apple notary service (profile: $NOTARY_PROFILE)…"
    xcrun notarytool submit "$APP_ZIP" \
        --keychain-profile "$NOTARY_PROFILE" \
        --wait

    echo
    echo "Stapling app ticket…"
    xcrun stapler staple "$APP_BUNDLE"
    xcrun stapler validate "$APP_BUNDLE"
    spctl -a -vvv -t execute "$APP_BUNDLE"
fi

echo
echo "Built app: $APP_BUNDLE"
