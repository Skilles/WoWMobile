#!/usr/bin/env bash
# Build benilla for a real iPhone, sign it with your Xcode account's team, install and launch it.
#
#   TEAM=ABCDE12345 ios/run-device.sh        # build, sign, install, launch
#   ios/run-device.sh --no-build             # re-sign and reinstall the last cargo build
#   ios/run-device.sh --copy-data            # also copy ~/wow/Data into the app's Documents/
#   ios/run-device.sh --no-launch            # install without launching
#
# Env: TEAM (your development team ID, from Xcode > Settings > Accounts; remembered in
# ios/.team after the first run), DEVICE (name or UDID; default: the one paired iPhone),
# BUNDLE_ID (default dev.benilla.ios), PROFILE (default release), WOW_DATA (default
# ~/wow/Data), CARGO_FEATURES.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=aarch64-apple-ios
PROFILE="${PROFILE:-release}"
BUNDLE_ID="${BUNDLE_ID:-dev.benilla.ios}"
WOW_DATA="${WOW_DATA:-$HOME/wow/Data}"
TEAM_FILE="$ROOT/ios/.team"

build=1
copy_data=0
launch=1
for arg in "$@"; do
    case "$arg" in
        --no-build) build=0 ;;
        --copy-data) copy_data=1 ;;
        --no-launch) launch=0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

if [[ -z "${TEAM:-}" && -f "$TEAM_FILE" ]]; then
    TEAM="$(cat "$TEAM_FILE")"
fi
if [[ -z "${TEAM:-}" ]]; then
    echo "Set TEAM to your development team ID (Xcode > Settings > Accounts > your team)." >&2
    exit 2
fi
echo "$TEAM" > "$TEAM_FILE"

if [[ -z "${DEVICE:-}" ]]; then
    DEVICE="$(xcrun devicectl list devices 2>/dev/null \
        | awk '/physical/ && /connected/ { for (i = 1; i <= NF; i++) { if ($i ~ /^[0-9A-F]{8}-[0-9A-F]{16}$/) { print $i; exit } } }')"
fi
if [[ -z "$DEVICE" ]]; then
    echo "No paired, connected iPhone found (xcrun devicectl list devices)." >&2
    exit 1
fi

if [[ $build == 1 ]]; then
    # shellcheck disable=SC2086
    cargo build --profile "$PROFILE" -p benilla --target "$TARGET" ${CARGO_FEATURES:-}
fi

# The app icon: the install's own logo, made once into a gitignored asset catalog.
ICONSET="$ROOT/ios/Assets.xcassets/AppIcon.appiconset"
if [[ ! -f "$ICONSET/icon.png" ]]; then
    mkdir -p "$ICONSET"
    WOW_DATA="$WOW_DATA" cargo run -q --release -p benilla-formats --example ios_icon -- "$ICONSET/icon.png"
    echo '{"info":{"author":"xcode","version":1}}' > "$ROOT/ios/Assets.xcassets/Contents.json"
    cat > "$ICONSET/Contents.json" <<'JSON'
{"images":[{"filename":"icon.png","idiom":"universal","platform":"ios","size":"1024x1024"}],"info":{"author":"xcode","version":1}}
JSON
fi

(cd "$ROOT/ios" && xcodegen generate --quiet --spec project.yml)

DERIVED="$ROOT/target/xcode"
xcodebuild \
    -project "$ROOT/ios/benilla.xcodeproj" \
    -scheme benilla \
    -configuration Release \
    -destination "id=$DEVICE" \
    -derivedDataPath "$DERIVED" \
    -allowProvisioningUpdates \
    -allowProvisioningDeviceRegistration \
    DEVELOPMENT_TEAM="$TEAM" \
    BENILLA_BUNDLE_ID="$BUNDLE_ID" \
    BENILLA_PROFILE="$PROFILE" \
    CURRENT_PROJECT_VERSION="$(date +%Y%m%d%H%M)" \
    -quiet \
    build

APP="$DERIVED/Build/Products/Release-iphoneos/benilla.app"
xcrun devicectl device install app --device "$DEVICE" "$APP"

if [[ $copy_data == 1 ]]; then
    echo "Copying $WOW_DATA to the app's Documents/Data (5 GB; takes a while over Wi-Fi)..."
    xcrun devicectl device copy to --device "$DEVICE" \
        --domain-type appDataContainer --domain-identifier "$BUNDLE_ID" \
        --source "$WOW_DATA" --destination Documents/Data
fi

if [[ $launch == 1 ]]; then
    xcrun devicectl device process launch --device "$DEVICE" --terminate-existing "$BUNDLE_ID"
fi
