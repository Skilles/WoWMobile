#!/usr/bin/env bash
# Build benilla for the iOS Simulator, wrap it in an .app and launch it.
#
#   ios/run-sim.sh                       # build, install, launch, stream the log
#   ios/run-sim.sh --no-build            # reinstall and relaunch the last build
#
# The Simulator reads the Mac's filesystem, so WOW_DATA can point at the Mac's copy of Data/.
# Env (all optional): WOW_DATA (default ~/wow/Data), WOW_HOST, SIM_DEVICE (name or UDID),
# PROFILE (default release), CARGO_FEATURES (extra cargo flags, e.g. --no-default-features).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=aarch64-apple-ios-sim
PROFILE="${PROFILE:-release}"
BUNDLE_ID="${BUNDLE_ID:-dev.benilla.ios}"
SIM_DEVICE="${SIM_DEVICE:-benilla iPhone 16 Pro Max}"
WOW_DATA="${WOW_DATA:-$HOME/wow/Data}"
APP="$ROOT/target/$TARGET/$PROFILE/benilla.app"

build=1
if [[ "${1:-}" == "--no-build" ]]; then
    build=0
fi

if [[ $build == 1 ]]; then
    # shellcheck disable=SC2086
    cargo build --profile "$PROFILE" -p benilla --target "$TARGET" ${CARGO_FEATURES:-}
fi

bin="$ROOT/target/$TARGET/$PROFILE/benilla"
rm -rf "$APP"
mkdir -p "$APP"
cp "$bin" "$APP/benilla"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
sed -e "s/\$(PRODUCT_BUNDLE_IDENTIFIER)/$BUNDLE_ID/" \
    -e "s/\$(MARKETING_VERSION)/$version/" \
    -e "s/\$(CURRENT_PROJECT_VERSION)/$(date +%Y%m%d%H%M)/" \
    "$ROOT/ios/Info.plist" > "$APP/Info.plist"
plutil -insert CFBundleSupportedPlatforms -json '["iPhoneSimulator"]' "$APP/Info.plist"
plutil -lint -s "$APP/Info.plist"
codesign --force --sign - --timestamp=none "$APP"

if ! xcrun simctl list devices booted | grep -q "$SIM_DEVICE"; then
    xcrun simctl boot "$SIM_DEVICE" 2>/dev/null || true
fi
if [[ -z "${NO_SIM_WINDOW:-}" ]]; then
    open "$(xcode-select -p)/Applications/Simulator.app" 2>/dev/null || true
fi
xcrun simctl bootstatus "$SIM_DEVICE" -b >/dev/null

xcrun simctl terminate "$SIM_DEVICE" "$BUNDLE_ID" 2>/dev/null || true
xcrun simctl install "$SIM_DEVICE" "$APP"

# SIMCTL_CHILD_* becomes the app's environment.
export SIMCTL_CHILD_WOW_DATA="$WOW_DATA"
export SIMCTL_CHILD_BENILLA_HOME="${BENILLA_HOME:-$ROOT/benilla-config-ios}"
if [[ -n "${WOW_HOST:-}" ]]; then
    export SIMCTL_CHILD_WOW_HOST="$WOW_HOST"
fi
export SIMCTL_CHILD_RUST_BACKTRACE=1
exec xcrun simctl launch --console-pty --terminate-running-process "$SIM_DEVICE" "$BUNDLE_ID"
