# Slice 7 handoff — Hardening + beta polish scaffold

**Branch:** `companion/slice-2-ios-shell`  
**Author model:** GPT-5 Codex  
**Date:** 2026-05-22

## What shipped

- Added feedback route and DTOs:
  - `POST /v1/feedback` in companion router/handler
  - `FeedbackRequest` / `FeedbackResponse` in Rust + FNDRKit
  - `CompanionClient.submitFeedback(request:)`
- Added app hardening scaffolds:
  - `SettingsView` app-lock and cache mode controls (`AppStorage`)
  - App Intents stubs in `apps/ios/FNDR/AppIntents/FNDRIntents.swift`
- Updated docs:
  - `docs/decisions/009-mobile-pairing-trust-model.md`
  - `docs/companion/api-contract.md`
  - `docs/companion/STATUS.md`

## Verification

- `cargo test companion:: -- --nocapture` ✅
- `swift run FNDRKitCheck` ✅

## Remaining validation gap

- Full security/polish acceptance (Face ID behavior, Spotlight/Siri, sleep/reconnect, TestFlight upload) requires full Xcode/device workflows and was not executable on this host.
