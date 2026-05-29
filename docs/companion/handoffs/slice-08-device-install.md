# Slice 8 handoff - real-device install readiness

**Branch:** current working branch  
**Date:** 2026-05-27

## What changed

- Enabled the iPhone target to build the existing Ask, Memories, Capture, and
  WatchBridge sources instead of hiding them behind slice-2 exclusions.
- Retained `PhoneWatchBridge` from app startup so Apple Watch requests can relay
  through the paired iPhone.
- Added camera QR pairing in `PairingView` with the required iOS camera usage
  string.
- Made iOS/watch signing configurable through `FNDR_DEVELOPMENT_TEAM` and
  `FNDR_BUNDLE_PREFIX`.
- Added `scripts/ios/install-real-device.sh` for signed device builds and
  `devicectl` installation.

## Verification

- `cd apps/ios/FNDRKit && swift run FNDRKitCheck` passed: 47 checks.
- `xcodebuild -project apps/ios/FNDR.xcodeproj -scheme FNDR -destination 'generic/platform=iOS' -derivedDataPath build/xcode-generic-ios CODE_SIGNING_ALLOWED=NO build` passed.
- `xcodebuild -project apps/ios/FNDR.xcodeproj -scheme 'FNDR Watch' -destination 'generic/platform=watchOS' -derivedDataPath build/xcode-generic-watch CODE_SIGNING_ALLOWED=NO build` passed.
- `bash -n scripts/ios/install-real-device.sh` passed.

## Remaining hardware step

`xcrun devicectl list devices` returned `No devices found`, so this session could
not perform the final signed install or live smoke on actual hardware. To close
the loop, connect/trust the iPhone, provide the team id and device id, then run:

```bash
FNDR_IOS_TEAM_ID=<apple-team-id> \
FNDR_IOS_BUNDLE_PREFIX=com.<your-name>.fndr \
FNDR_IOS_DEVICE_ID=<iphone-device-id> \
scripts/ios/install-real-device.sh
```

After install, start `npm run tauri dev` on the Mac, generate the pairing QR in
FNDR Settings -> Paired devices, scan it from the iPhone, and smoke Status, Ask,
Memories, Capture, and the watch app.
