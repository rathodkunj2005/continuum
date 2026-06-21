# Continuum Companion API + Mobile

The Companion API is the local-network HTTP surface that the Continuum iPhone
and Apple Watch apps talk to. The Mac runs Continuum as usual; mobile clients
ask, search, capture notes, and pause capture by calling `/v1/...` over
local Wi-Fi.

## Reading order

1. [009-companion-api-architecture.md](../decisions/009-companion-api-architecture.md) — why this exists and how it's shaped.
2. [api-contract.md](./api-contract.md) — versioned route reference + `curl` smoke pack.
3. [STATUS.md](./STATUS.md) — what's shipped vs. open across the 7-slice roadmap.
4. [handoffs/](./handoffs/) — per-slice handoff notes (read the highest-numbered file first).

## Layout

- Rust: `src-tauri/src/companion/`
- Tauri commands for the desktop React UI: `src-tauri/src/ipc/commands/companion.rs`
- React Settings panel: `src/domains/companion/CompanionDevicesPanel.tsx`
- iOS / watchOS apps: `apps/ios/`

## How to run + test locally

```bash
# Backend
cd src-tauri
cargo test --lib companion          # focused unit tests
cargo test --lib                    # full Rust suite

# Frontend
npm run typecheck
npx vitest run src/domains/companion

# ContinuumKit package checks (CLI-safe, no XCTest dependency)
cd ../apps/ios/ContinuumKit
swift run ContinuumKitCheck

# End-to-end smoke
cd ../../
npm run tauri dev                   # the companion API starts on a random port
cat ~/.continuum/companion.json          # discovery file with host/port
# then follow api-contract.md's curl pack
```
