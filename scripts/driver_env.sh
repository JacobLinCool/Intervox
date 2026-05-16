#!/usr/bin/env bash
# Shared config for the Intervox HAL driver build/sign/notarize/install
# scripts. Override any of these via the environment.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export REPO_ROOT

# Code-signing identity (Developer ID Application). `security find-identity
# -v -p codesigning` lists valid identities.
: "${SIGN_IDENTITY:=Developer ID Application: JHEN-KE LIN (5H75SGA7KP)}"
: "${TEAM_ID:=5H75SGA7KP}"

# notarytool keychain profile. Create once (secrets stay in your keychain):
#   xcrun notarytool store-credentials intervox-notary \
#     --apple-id <id> --team-id 5H75SGA7KP --password <app-specific-pw>
: "${NOTARY_PROFILE:=intervox-notary}"

DRIVER_DIR="$REPO_ROOT/driver"
BUILD_DIR="$DRIVER_DIR/build"
DRIVER_BUNDLE="$BUILD_DIR/Intervox.driver"
HAL_DIR="/Library/Audio/Plug-Ins/HAL"
INSTALLED_BUNDLE="$HAL_DIR/Intervox.driver"

export SIGN_IDENTITY TEAM_ID NOTARY_PROFILE DRIVER_DIR BUILD_DIR \
    DRIVER_BUNDLE HAL_DIR INSTALLED_BUNDLE
