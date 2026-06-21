# Slice 2 handoff — iOS shell + pairing

**Branch:** `companion/slice-2-ios-shell`  
**Author model:** GPT-5 Codex  
**Date:** 2026-05-26

## Slice-2 scope now enforced

Kept in this branch:
- `apps/ios/` project scaffold (`Continuum.xcodeproj`, `project.yml`, SwiftUI shell)
- `ContinuumKit` pairing/client foundations
- Pairing flow (`PairingFlow`) with Keychain token persistence
- Status-tab connectivity (`/v1/status`, `/v1/capture/control`)

Explicitly peeled out of runtime surface for this slice:
- Ask tab UX (slice 3)
- Memories search/detail UX (slice 4)
- Capture tab + offline queue UX (slice 5)
- Watch relay activation from iOS app startup (slice 6)
- Settings hardening controls and related UX scaffolding (slice 7)

## Companion API smoke evidence (desktop live service)

Executed against live `npm run tauri dev` endpoint from `~/.continuum/companion.json`:
- `GET /v1/health` ✅
- `POST /v1/pair/start` ✅
- `POST /v1/pair/complete` ✅
- `GET /v1/status` ✅
- `POST /v1/capture/control` pause/resume ✅
- `POST /v1/memories/manual` ✅

Durable fixes made during smoke:
- Axum route syntax fix to avoid startup panic:
  - `src-tauri/src/companion/mod.rs`
  - `/v1/memories/:memory_id` -> `/v1/memories/{memory_id}`
- Lance writer compatibility for existing memory schemas:
  - `src-tauri/src/storage/lance_store/schemas.rs`
  - `src-tauri/src/storage/lance_store/arrow_and_filters.rs`
  - `src-tauri/src/storage/lance_store/normalize_embed_migrate.rs`

## Verification run in this pass

- Live smoke flow above against running Tauri app ✅
- Rust app rebuilt successfully under `npm run tauri dev` after changes ✅

## Remaining to close slice-2 acceptance on device/simulator

1. Build and run `apps/ios/Continuum` in Xcode on simulator/device.
2. Complete pairing from pasted/scanned QR payload and confirm Keychain persistence across app relaunch.
3. Verify Status tab refresh and pause/resume against a live desktop companion endpoint.

## Follow-ups for next slices (out of scope here)

1. Slice 3: Ask tab and `/v1/ask` UX polish + runtime validation.
2. Slice 4: memory search/detail UX + runtime validation.
3. Slice 5: manual capture UX + durable offline queue and retry behavior.
4. Slice 6: watch relay activation and watch simulator/device validation.
5. Slice 7: hardening controls, permissions UX, and privacy/polish.
