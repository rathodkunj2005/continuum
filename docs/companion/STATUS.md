# Continuum Mobile Companion — slice status board

Treat this as the single source of truth for "what's shipped, what's
next." Every slice ends by flipping its row to ✅ and writing a fresh
`handoffs/slice-NN.md`. The next session reads this file first.

| #  | Slice                                    | Branch                                       | Status | Notes                                                                                  |
|----|------------------------------------------|----------------------------------------------|--------|----------------------------------------------------------------------------------------|
| 1  | Companion API foundation (Rust)          | `companion/slice-1-api-foundation`           | ✅      | Pair/status/capture-control/manual-memory + device registry + React Settings panel, verified with companion Rust tests. |
| 2  | iOS shell + pairing                      | `companion/slice-2-ios-shell`                | 🟡      | `ContinuumKit`, `Continuum.xcodeproj`, QR pairing, Keychain token persistence, and Status-tab connectivity are in the device build. Physical-device smoke is still pending. |
| 3  | Ask Continuum on iPhone                       | `companion/slice-3-ios-ask`                  | 🟡      | `/v1/ask` route + ContinuumKit/client + Ask tab are included in the device build; live device runtime evidence is still pending. |
| 4  | Memory search + detail                   | `companion/slice-4-ios-search`               | 🟡      | `/v1/memories/search` uses canonical hybrid retrieval and the iOS memory tab is included in the device build; live device runtime evidence is still pending. |
| 5  | Manual capture + offline queue           | `companion/slice-5-ios-capture`              | 🟡      | Manual capture + durable queue are included in the device build with idempotency; real offline-retry smoke is still pending. |
| 6  | Apple Watch MVP                          | `companion/slice-6-watch`                    | 🟡      | WatchConnectivity relay is retained by the iPhone app at startup and the watch app is built for device; watch device validation is still pending. |
| 7  | Hardening + beta polish                  | `companion/slice-7-hardening`                | 🟡      | Permission-scoped auth, provenance-spoof rejection, and feedback redaction landed; full iOS/watch runtime hardening validation still pending. |
| 8  | Real-device install readiness            | current working branch                       | 🟡      | Device-install script, configurable signing, QR camera pairing, full phone tabs, and generic iPhone/watch builds pass. Physical install is blocked until an iPhone/watch is connected and trusted. |

## Reference

- PRD: `~/Downloads/continuum_ios_watch_mvp_prd.md`
- Plan: `~/.claude/plans/users-anurupkumar-downloads-continuum-ios-wa-melodic-starfish.md`
- ADR: [009-companion-api-architecture.md](../decisions/009-companion-api-architecture.md)
- ADR: [009-mobile-pairing-trust-model.md](../decisions/009-mobile-pairing-trust-model.md)
- API contract: [api-contract.md](./api-contract.md)
- Latest handoff: [handoffs/slice-08-device-install.md](./handoffs/slice-08-device-install.md)

## Cross-cutting decisions locked at slice 1

- iOS project root: `apps/ios/` (top-level `apps/`, no rename of `src-tauri/` yet).
- Companion API: separate Axum router, sibling port to MCP, same TLS cert.
- Git cadence: one PR per slice, merge to `main` as each lands.
- Networking: local-network only; QR encodes endpoint + 6-digit code; no mDNS.
- Auth: opaque 256-bit access tokens; revocable; no JWT.

## Known issues outside the slice scope

- Pre-existing Vitest failures in `src/shared/theme/__tests__/wallpaper-field-colors.test.ts`,
  `cinematic-palettes.test.ts`, and `wallpaper-registry.test.ts` — unrelated to
  companion. Filed as a separate task.

## Device install path

- Real-device install script: `scripts/ios/install-real-device.sh`
- Signing is configured through `CONTINUUM_DEVELOPMENT_TEAM` and
  `CONTINUUM_BUNDLE_PREFIX`, so the same project can build with a personal Apple
  team and unique bundle id without editing `project.yml`.
- The iPhone app now includes camera QR pairing plus local-network permission
  text for real device pairing against the Mac companion endpoint.

## Validation blocker

- Full Xcode is selected at `/Applications/Xcode.app/Contents/Developer`.
- No physical iPhone or Apple Watch was visible to `xcrun devicectl list
  devices` in the device-install session, so the remaining acceptance gap is a
  signed install plus live pairing/status/ask/search/capture/watch smoke on
  actual hardware.
