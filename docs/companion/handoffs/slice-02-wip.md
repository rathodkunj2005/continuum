# Slice 2 WIP handoff — iOS shell + pairing

> Superseded by `docs/companion/handoffs/slice-02.md`.

**Branch:** `companion/slice-2-ios-shell`  
**Author model:** GPT-5 Codex  
**Date:** 2026-05-22

## What was completed in this pass

- Stabilized and shipped `apps/ios/ContinuumKit` as a runnable Swift package in
  this environment.
- Kept the custom non-XCTest harness (`ContinuumKitCheck`) and fixed compile issues:
  - assertion helpers now accept throwing autoclosures,
  - `StatusResponse` has a public initializer for tests/suites,
  - `URLSessionTransport.derToPEM` is public for pinning tests.
- Added conditional XCTest compilation (`#if canImport(XCTest)`) in
  `apps/ios/ContinuumKit/Tests/ContinuumKitTests/*.swift` so `swift test` does not fail
  on machines without XCTest while preserving the full XCTest suite for Xcode.
- Added draft SwiftUI app-shell files under `apps/ios/Continuum/`:
  - `App/` session + tab shell,
  - `Pairing/` payload parse + complete flow draft wired to `PairingFlow`,
  - `Status/` refresh + pause/resume draft wired to `ConnectionStatusService`
    and `capture/control`,
  - placeholder Ask/Memories/Capture/Settings views.
- Added XcodeGen project spec + generated app project:
  - `apps/ios/project.yml`
  - `apps/ios/Continuum.xcodeproj`
- Added `apps/ios/README.md` with regenerate/build/smoke instructions and
  environment notes.
- Companion integration recovery + correctness hardening landed (no feature expansion):
  - Companion now binds on LAN-reachable host by default and advertises a
    resolved reachable host into `~/.continuum/companion.json`.
  - `/v1/memories/search` now uses `HybridSearcher::search_hybrid_memories`
    (canonical server-side retrieval path) instead of `ComposeMode::Cards`.
  - Route-level permission enforcement added in auth middleware with typed
    `insufficient_permission` error responses.
  - Manual capture rejects provenance spoofing via `source_override` mismatch.
  - Feedback logging now emits redacted metadata only (no raw query text).
  - WatchConnectivity bridge now routes watch requests through iPhone
    `CompanionSession`/`CompanionClient` (no direct watch-to-Mac calls).

## Verification run

- `cd src-tauri && cargo test companion:: -- --nocapture` ✅
  - 46/46 companion tests passing.
- `cd apps/ios/ContinuumKit && swift run ContinuumKitCheck` ✅
  - 46/46 suite cases passing.
- `cd apps/ios/ContinuumKit && swift test` ✅ in this environment
  - No XCTest available, so test target builds and runs 0 test cases (by
    design due `#if canImport(XCTest)` guards).
- `xcodegen generate --spec apps/ios/project.yml --project apps/ios` ✅
  - Project generated at `apps/ios/Continuum.xcodeproj`.
- `ls apps/ios/Continuum.xcodeproj/xcshareddata/xcschemes` ✅
  - Shared scheme present: `Continuum.xcscheme`.

## Remaining for slice-2 acceptance

1. Validate on iOS simulator/device:
   - QR scan or payload entry,
   - pair complete and token persistence in Keychain,
   - Status tab live refresh against `/v1/status`.
2. Validate watch relay on simulator/device:
   - Ask/Remember/Recent/Status calls succeed through WatchConnectivity bridge.
3. Validate discovery file after desktop runtime start:
   - `~/.continuum/companion.json` exists and advertises reachable host (not hardcoded loopback).

## Environment blocker (Codex host)

- Full Xcode toolchain is not installed/selected in this runtime.
- Observed command failures:
  - `xcodebuild -version` -> requires full Xcode (active developer dir is
    `/Library/Developer/CommandLineTools`)
  - `xcrun simctl list devices` -> `simctl` not found
- Result: simulator/device smoke could not be executed from this session.
- Next step on a full-Xcode machine:
  - `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`
  - `xcodebuild -project apps/ios/Continuum.xcodeproj -scheme Continuum -destination 'platform=iOS Simulator,name=iPhone 15' build`
  - then run the pairing/status smoke in `apps/ios/README.md`.

## Known unrelated baseline

- Pre-existing modified files outside this slice (left untouched):
  - `src/shared/theme/__tests__/cinematic-palettes.test.ts`
  - `src/shared/theme/__tests__/wallpaper-field-colors.test.ts`
  - `src/shared/wallpaper/__tests__/wallpaper-registry.test.ts`
