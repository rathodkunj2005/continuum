#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT="$ROOT/apps/ios/FNDR.xcodeproj"
DERIVED_DATA="${FNDR_IOS_DERIVED_DATA:-$ROOT/build/ios-device}"
CONFIGURATION="${FNDR_IOS_CONFIGURATION:-Debug}"
BUNDLE_PREFIX="${FNDR_IOS_BUNDLE_PREFIX:-com.fndr.ios}"
TEAM_ID="${FNDR_IOS_TEAM_ID:-}"
IPHONE_DEVICE_ID="${FNDR_IOS_DEVICE_ID:-}"
WATCH_DEVICE_ID="${FNDR_WATCH_DEVICE_ID:-}"

usage() {
  cat <<'USAGE'
Install FNDR on a real iPhone, with the embedded watchOS app built for Apple Watch.

Required:
  FNDR_IOS_TEAM_ID       Apple development team id used for signing.
  FNDR_IOS_DEVICE_ID     Connected/trusted iPhone identifier from `xcrun devicectl list devices`.

Optional:
  FNDR_IOS_BUNDLE_PREFIX Unique bundle prefix, default: com.fndr.ios
  FNDR_WATCH_DEVICE_ID   Connected Apple Watch identifier if you want direct watch install too.

Example:
  FNDR_IOS_TEAM_ID=ABCDE12345 \
  FNDR_IOS_BUNDLE_PREFIX=com.anurup.fndr \
  FNDR_IOS_DEVICE_ID=00008130-001234... \
  scripts/ios/install-real-device.sh
USAGE
}

if [[ -z "$TEAM_ID" || -z "$IPHONE_DEVICE_ID" ]]; then
  usage
  echo
  echo "Visible devices:"
  xcrun devicectl list devices || true
  exit 2
fi

COMMON_SETTINGS=(
  "FNDR_DEVELOPMENT_TEAM=$TEAM_ID"
  "FNDR_BUNDLE_PREFIX=$BUNDLE_PREFIX"
  "CODE_SIGN_STYLE=Automatic"
)

echo "Building FNDR for iPhone device $IPHONE_DEVICE_ID"
xcodebuild \
  -project "$PROJECT" \
  -scheme FNDR \
  -configuration "$CONFIGURATION" \
  -destination "id=$IPHONE_DEVICE_ID" \
  -derivedDataPath "$DERIVED_DATA" \
  -allowProvisioningUpdates \
  "${COMMON_SETTINGS[@]}" \
  build

IPHONE_APP="$DERIVED_DATA/Build/Products/$CONFIGURATION-iphoneos/FNDR.app"
if [[ ! -d "$IPHONE_APP" ]]; then
  echo "Expected iPhone app not found: $IPHONE_APP" >&2
  exit 1
fi

echo "Installing FNDR on iPhone"
xcrun devicectl device install app --device "$IPHONE_DEVICE_ID" "$IPHONE_APP"

if [[ -n "$WATCH_DEVICE_ID" ]]; then
  echo "Building FNDR Watch for watch device $WATCH_DEVICE_ID"
  xcodebuild \
    -project "$PROJECT" \
    -scheme "FNDR Watch" \
    -configuration "$CONFIGURATION" \
    -destination "id=$WATCH_DEVICE_ID" \
    -derivedDataPath "$DERIVED_DATA" \
    -allowProvisioningUpdates \
    "${COMMON_SETTINGS[@]}" \
    build

  WATCH_APP="$DERIVED_DATA/Build/Products/$CONFIGURATION-watchos/FNDR Watch.app"
  if [[ ! -d "$WATCH_APP" ]]; then
    echo "Expected watch app not found: $WATCH_APP" >&2
    exit 1
  fi

  echo "Installing FNDR Watch"
  xcrun devicectl device install app --device "$WATCH_DEVICE_ID" "$WATCH_APP"
fi

echo "Install complete. Start FNDR on the Mac, generate a companion QR code, then pair from the iPhone app."
