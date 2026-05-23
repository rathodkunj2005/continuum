# Slice 3 handoff — Ask FNDR on iPhone

**Branch:** `companion/slice-2-ios-shell`  
**Author model:** GPT-5 Codex  
**Date:** 2026-05-22

## What shipped

- Added Companion API `POST /v1/ask` route:
  - Rust handler: `src-tauri/src/companion/handlers/ask.rs`
  - DTOs: `AskRequest`, `AskResponse`, `CompanionMemoryCard`
- Route mounted in `src-tauri/src/companion/mod.rs` under authenticated router.
- Added iOS/FNDRKit support:
  - `CompanionClient.ask(request:)`
  - DTO decode coverage in `FNDRKitCheck`.
- Added Ask tab implementation:
  - `apps/ios/FNDR/Ask/AskViewModel.swift`
  - `apps/ios/FNDR/Ask/AskView.swift`
  - query history + answer style selector + source-card list.

## Verification

- `cd src-tauri && cargo test companion:: -- --nocapture` ✅
- `cd apps/ios/FNDRKit && swift run FNDRKitCheck` ✅

## Remaining validation gap

- iOS simulator live Ask smoke is blocked in Codex host due missing full Xcode/simctl.
