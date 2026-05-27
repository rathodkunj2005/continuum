# FNDR Mobile Companion — slice status board

Treat this as the single source of truth for "what's shipped, what's
next." Every slice ends by flipping its row to ✅ and writing a fresh
`handoffs/slice-NN.md`. The next session reads this file first.

| #  | Slice                                    | Branch                                       | Status | Notes                                                                                  |
|----|------------------------------------------|----------------------------------------------|--------|----------------------------------------------------------------------------------------|
| 1  | Companion API foundation (Rust)          | `companion/slice-1-api-foundation`           | ✅      | Pair/status/capture-control/manual-memory + device registry + React Settings panel, verified with companion Rust tests. |
| 2  | iOS shell + pairing                      | `companion/slice-2-ios-shell`                | 🟡      | `FNDRKit`, `FNDR.xcodeproj`, pairing + Keychain token persistence + Status-tab connectivity landed as a clean slice-2 surface. Ask/search/capture-queue/watch/hardening are deferred to slices 3-7. |
| 3  | Ask FNDR on iPhone                       | `companion/slice-3-ios-ask`                  | 🟡      | `/v1/ask` route + FNDRKit/client + Ask tab landed; no full-Xcode simulator/device runtime evidence yet. |
| 4  | Memory search + detail                   | `companion/slice-4-ios-search`               | 🟡      | `/v1/memories/search` now uses canonical hybrid retrieval path; iOS memory flows need full-Xcode E2E validation. |
| 5  | Manual capture + offline queue           | `companion/slice-5-ios-capture`              | 🟡      | Manual capture + durable queue landed with idempotency; simulator/device offline-retry smoke still pending. |
| 6  | Apple Watch MVP                          | `companion/slice-6-watch`                    | 🟡      | WatchConnectivity relay wiring landed so Watch routes through iPhone; watch simulator/device validation pending. |
| 7  | Hardening + beta polish                  | `companion/slice-7-hardening`                | 🟡      | Permission-scoped auth, provenance-spoof rejection, and feedback redaction landed; full iOS/watch runtime hardening validation still pending. |

## Reference

- PRD: `~/Downloads/fndr_ios_watch_mvp_prd.md`
- Plan: `~/.claude/plans/users-anurupkumar-downloads-fndr-ios-wa-melodic-starfish.md`
- ADR: [009-companion-api-architecture.md](../decisions/009-companion-api-architecture.md)
- ADR: [009-mobile-pairing-trust-model.md](../decisions/009-mobile-pairing-trust-model.md)
- API contract: [api-contract.md](./api-contract.md)

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

## Validation blocker

- Full Xcode is now selected at `/Applications/Xcode.app/Contents/Developer`,
  but slice-2 still needs explicit simulator/device runtime evidence (pairing
  flow + Status tab against a live desktop companion endpoint).
