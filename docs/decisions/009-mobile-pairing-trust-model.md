# 009: Mobile pairing trust model for Companion API

Continuum mobile pairing must stay local-first, revocable, and simple enough to
operate without cloud identity.

## Decision

Use a two-step local pairing flow:

1. Mac generates a short-lived 6-digit pairing code and QR payload with
   endpoint metadata (`host`, `port`, `tls`, `cert_fingerprint_sha256`).
2. iPhone submits `POST /v1/pair/complete` with that code and device metadata.

On success, Mac issues an opaque random access token, persists a `MobileDevice`
entry in `StateStore` (`companion_devices`), and requires bearer auth on all
protected routes.

## Trust assumptions

- User is physically present at the Mac when initiating pairing.
- Local network path is potentially hostile, so TLS and pinning are mandatory.
- Token possession grants companion permissions until revoked.

## Security properties

- Pairing codes are single-use and expire (`PAIRING_TTL_MS`).
- Tokens are revocable at device granularity from Mac Settings.
- Revoked or unknown tokens return `403 forbidden`.
- Mobile apps pin to `cert_fingerprint_sha256` from QR payload.
- Pairing is local-network only for MVP; no relay or remote discovery.

## Out of scope (deferred)

- Per-request signatures with device key material.
- Token scope partitioning beyond current permission list.
- Cloud identity or account-linked device enrollment.
- Remote relay/NAT traversal.

## Consequences

- UX stays fast and offline-friendly for home/office usage.
- Revocation semantics are explicit and testable.
- Rotating the Mac TLS cert invalidates previous pin state and requires re-pair.
