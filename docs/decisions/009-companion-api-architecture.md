# 008: Companion API architecture

The Mac is the brain; mobile clients are thin. The iPhone / Apple Watch
companion apps need a stable, secure local-network surface to ask Continuum
questions, search memories, save manual notes, and control capture state —
without copying any of the runtime, models, or memory store onto the phone.

## Decision

Add a dedicated **Companion API** as a new Rust module
`src-tauri/src/companion/` that mounts its own Axum router on a sibling port
to the existing MCP server. Mobile clients pair via a short-lived 6-digit
code shown on the Mac, receive an opaque 256-bit access token, and hit
`/v1/...` routes guarded by a bearer-token middleware. Tokens are revocable
from the Mac UI.

## Why a separate module instead of extending MCP

The MCP server speaks a tool-protocol shape (JSON-RPC, SSE) and is shaped for
editor / agent clients. The mobile API is a user-facing app API with
different concerns: device pairing, per-device revocation, mobile-shaped
DTOs (no internal vector scores, no raw screenshots), and an explicit
permission allowlist. Sharing the same router would couple two surfaces that
should evolve independently. Sharing Axum, tokio, and the self-signed TLS
cert is enough convergence; route-level cohabitation is not.

## Why opaque random tokens instead of JWT

For MVP the only revocation we need is "this device is no longer trusted."
A lookup against an in-memory list backed by `StateStore` is simpler than a
JWT denylist and matches how MCP already issues its single bearer token.
JWT would buy us nothing until per-request signing or claims-based scoping
is needed; both are deferred (P2+).

## Why local-network only, no relay

PRD §6 lists remote relay as a non-goal for MVP. NAT traversal and a
hardened relay design are large additions to the threat model and add a
dependency the user cannot self-host. The MVP ships with QR-encoded
host/port for in-network reach; mDNS/Bonjour and remote relay are deferred
to P2.

## Why pair via QR + 6-digit code

QR encodes the full endpoint payload (`host`, `port`, `tls`,
`cert_fingerprint_sha256`, `pairing_code`) so the iPhone needs no manual IP
entry. The numeric code is a human-readable fallback for users without a
camera path (typing into the iOS app), and it doubles as a confirmation
that the user is in front of the Mac when pairing. Codes are single-use and
expire after 5 minutes (`PAIRING_TTL_MS`).

## Schema and persistence

A new key `companion_devices` in `StateStore` holds
`Vec<MobileDevice>`. The schema lives in
[src-tauri/src/companion/dto.rs](../../src-tauri/src/companion/dto.rs);
the registry implementation in
[src-tauri/src/companion/device_registry.rs](../../src-tauri/src/companion/device_registry.rs).
No change to the existing LanceDB tables. Mobile-origin memories use the
existing `MemoryRecord.source_type` field with new values
`"iphone_manual_capture"` and `"watch_manual_capture"`.

## Provenance and privacy

- Manual mobile memories always carry the authenticated device's
  `source_type` — clients cannot impersonate desktop captures.
- The Companion API never returns raw OCR or screenshot bytes (slice 1
  exposes no read endpoints for raw text; later slices return summarized
  memory cards only).
- Revocation propagates on the next request from the revoked device (401).

## Discovery file

The server writes `~/.continuum/companion.json` on start and removes it on stop,
mirroring the MCP discovery convention. iOS uses this in the future for
diagnostic surfaces; primary configuration still flows through QR pairing.

## Open questions deferred

- Streaming Ask responses (slice 3 will revisit; SSE is the likely answer).
- Per-permission allowlist on the token (today everyone gets all four).
- mDNS / Bonjour discovery (P2).
- Encrypted-at-rest token storage on Mac side (currently relies on
  filesystem permissions of `~/.continuum/`).
