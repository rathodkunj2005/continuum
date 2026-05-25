# FNDR Mobile Companion — slice status board

Treat this as the single source of truth for "what's shipped, what's
next." Every slice ends by flipping its row to ✅ and writing a fresh
`handoffs/slice-NN.md`. The next session reads this file first.

| #  | Slice                                    | Branch                                       | Status | Notes                                                                                  |
|----|------------------------------------------|----------------------------------------------|--------|----------------------------------------------------------------------------------------|
| 1  | Companion API foundation (Rust)          | `companion/slice-1-api-foundation`           | ✅      | Pair/status/capture-control/manual-memory + device registry + React Settings panel.    |
| 2  | iOS shell + pairing                      | `companion/slice-2-ios-shell`                | ⏳      | Bootstrap `apps/ios/` Xcode project; pair via QR; Keychain token; Status tab.          |
| 3  | Ask FNDR on iPhone                       | `companion/slice-3-ios-ask`                  | ⏳      | `/v1/ask` wraps `fndr_answer`; Ask tab with source-card list + detail.                 |
| 4  | Memory search + detail                   | `companion/slice-4-ios-search`               | ⏳      | `/v1/memories/search` wraps `search_hybrid_memories`; Memories tab.                    |
| 5  | Manual capture + offline queue           | `companion/slice-5-ios-capture`              | ⏳      | iOS SwiftData queue; idempotent retry via `client_event_id`.                           |
| 6  | Apple Watch MVP                          | `companion/slice-6-watch`                    | ⏳      | watchOS target + WatchConnectivity bridge; 4 screens.                                   |
| 7  | Hardening + beta polish                  | `companion/slice-7-hardening`                | ⏳      | App lock, App Intents, telemetry, sleep/reconnect, TestFlight prep.                    |

## Reference

- PRD: `~/Downloads/fndr_ios_watch_mvp_prd.md`
- Plan: `~/.claude/plans/users-anurupkumar-downloads-fndr-ios-wa-melodic-starfish.md`
- ADR: [009-companion-api-architecture.md](../decisions/009-companion-api-architecture.md)
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
