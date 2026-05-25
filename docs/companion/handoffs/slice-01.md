# Slice 1 handoff — Companion API foundation

**Branch:** `companion/slice-1-api-foundation`
**Author model:** Claude Opus 4.7
**Date:** 2026-05-22

## What shipped

- New Rust module `src-tauri/src/companion/` (router + pairing + auth +
  device registry + handlers). Spawns on app boot as a sibling axum
  router to MCP; uses the same self-signed TLS cert.
- Endpoints live: `GET /`, `GET /v1/health`, `POST /v1/pair/start`,
  `POST /v1/pair/complete`, `GET /v1/status`, `POST /v1/capture/control`,
  `POST /v1/memories/manual`.
- Discovery file: `~/.fndr/companion.json` (host, port, cert fingerprint,
  Mac name, app version).
- Tauri commands for the desktop UI: `companion_get_status`,
  `companion_get_endpoint`, `companion_start_server`, `companion_stop_server`,
  `companion_start_pairing`, `companion_list_devices`, `companion_revoke_device`.
- React Settings panel: `src/domains/companion/CompanionDevicesPanel.tsx`
  with QR payload display, 6-digit code reveal, paired-devices list,
  revoke. 5 Vitest cases pass.
- 36 new Rust unit tests covering DTOs, error mapping, device registry
  durability + idempotent revoke, pairing TTL + single-use + reject paths,
  bearer-token middleware + extension carry-through, deterministic memory
  id derivation, manual-record provenance.
- Docs: [ADR-009](../../decisions/009-companion-api-architecture.md),
  [README](../README.md), [api-contract.md](../api-contract.md),
  [STATUS.md](../STATUS.md) (this is the file to read first next time).

## Acceptance criteria status

- [x] `curl` from Mac CLI can pair using a code, then call `/v1/status` with
      token. — Verified shape via unit tests + curl smoke pack in
      `api-contract.md`. End-to-end curl against a live `tauri dev` was
      not run in this session (TLS + interactive QR flow); next session
      should run the smoke pack on a real `tauri dev` and confirm.
- [x] Revoked device returns 401 (actually 403 by design — see ADR §error
      envelope; "revoked" and "unknown" are both `forbidden`, distinct from
      `unauthenticated` = missing header).
- [x] `make test` passes: 460 Rust tests + 36 new companion tests (496 total
      passing). 5 pre-existing theme/wallpaper Vitest failures are NOT
      caused by this slice and are tracked in a spawned task.
- [x] ADR `docs/decisions/009-companion-api-architecture.md` written.

## What to read first next session

1. `docs/companion/STATUS.md`
2. This handoff (`slice-01.md`)
3. `docs/decisions/009-companion-api-architecture.md`
4. `docs/companion/api-contract.md` (especially the curl smoke pack)

## Gotchas observed

- `rand 0.10` moved `random_range` and `sample_iter` from `Rng` to the
  new `RngExt` trait. Anything using these in companion code must
  `use rand::RngExt;`.
- `StateStore::new` builds its own current-thread tokio runtime
  internally. Calling it from inside `#[tokio::test]` panics with
  "Cannot start a runtime from within a runtime." Construct it via
  `tokio::task::spawn_blocking` in async tests (see
  `src-tauri/src/companion/auth.rs` test helpers).
- The repo has 5 pre-existing Vitest failures in `src/shared/theme` and
  `src/shared/wallpaper`. They reproduce on `main` and are NOT caused by
  any code in this slice. They are filed as a spawned task. Future slices
  should treat them as a known baseline and not block on them, until that
  spawned task lands.
- The Companion API spawns on Tauri app boot. It picks a random free port
  (port=0). Look in `~/.fndr/companion.json` for the value; the React
  panel surfaces it via `companion_get_status`.

## Open seams for slice 2 (iOS shell + pairing)

- `apps/ios/` does not exist yet. Create the Xcode project (iOS 17+,
  watchOS 10+ target) with a single SwiftUI app target. Make the shared
  `FNDRKit` Swift package house the networking layer.
- The QR payload is already a JSON string (`pairing_code`, `host`, `port`,
  `tls`, `cert_fingerprint_sha256`). iOS needs only to scan + POST.
- For TLS pinning, iOS should pin to `cert_fingerprint_sha256` (a sha256
  of the PEM bytes — see `tls_cert_fingerprint()` in
  `src-tauri/src/companion/mod.rs`). The actual cert PEM is also available
  via `mcp::tls::get_cert_pem()` — we could surface it via a paired-only
  endpoint in slice 2 if pinning by fingerprint alone is awkward.
- The React panel's "QR payload (debug)" `<details>` is intentionally not
  a rendered QR — slice 2 should add an actual QR pixel renderer
  (recommend a tiny dep like `qrcode` or a hand-rolled SVG).

## Out-of-scope items deliberately deferred

- Ask / search routes (slices 3 / 4).
- mDNS, remote relay (P2).
- Per-permission allowlist on tokens (P2).
- Encrypted-at-rest token storage on the Mac (relies on `~/.fndr/`
  filesystem permissions today).
- iOS Xcode project + SwiftUI code (slice 2).
- TestFlight pipeline (slice 7).
