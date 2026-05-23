# Slice 5 handoff — Manual capture + offline queue

**Branch:** `companion/slice-2-ios-shell`  
**Author model:** GPT-5 Codex  
**Date:** 2026-05-22

## What shipped

- Added durable offline queue in FNDRKit:
  - `apps/ios/FNDRKit/Sources/FNDRKit/OfflineCaptureQueue.swift`
  - enqueue durability, retry, and idempotent `client_event_id` behavior.
- Added capture UI wiring:
  - `apps/ios/FNDR/Capture/CaptureViewModel.swift`
  - `apps/ios/FNDR/Capture/CaptureView.swift`
  - direct save → queue fallback → manual flush action.
- Extended session boundary with queue helpers:
  - `apps/ios/FNDR/App/CompanionSession.swift`

## Verification

- `swift run FNDRKitCheck` suite includes `OfflineCaptureQueue` tests ✅
  - durable reload across restart
  - success flush drains queue
  - failed flush increments attempts

## Remaining validation gap

- Live iOS airplane-mode / reconnect smoke not run here due simulator tooling blocker.
