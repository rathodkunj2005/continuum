# Companion API v1 — route reference

Base URL: `https://<host>:<port>/v1` (TLS is mandatory; the cert is the
same self-signed cert the MCP server uses, written to
`~/.fndr/mcp_cert.pem`). Read `~/.fndr/companion.json` for the live
`host`, `port`, and `cert_fingerprint_sha256`.

Auth: `Authorization: Bearer <access_token>` on every route EXCEPT
`/v1/pair/start` and `/v1/pair/complete`. Tokens are issued during pairing
and revocable from the Mac UI.

| Route                       | Auth | Slice | Notes                                  |
| --------------------------- | ---- | ----- | -------------------------------------- |
| `GET  /`                    | no   | 1     | service descriptor                     |
| `GET  /v1/health`           | no   | 1     | liveness                               |
| `POST /v1/pair/start`       | no   | 1     | Mac UI starts one-time code + QR       |
| `POST /v1/pair/complete`    | no   | 1     | one-shot; consumes the code            |
| `GET  /v1/status`           | yes  | 1     | capture + runtime status               |
| `POST /v1/capture/control`  | yes  | 1     | pause / resume / incognito             |
| `POST /v1/memories/manual`  | yes  | 1     | text capture from phone/watch          |
| `POST /v1/ask`              | yes  | 3     | Ask FNDR                               |
| `POST /v1/memories/search`  | yes  | 4     | cards-oriented memory search           |
| `GET  /v1/memories/:id`     | yes  | 4     | memory detail card                     |
| `POST /v1/feedback`         | yes  | 7     | thumbs / open-source events            |

## Schemas

See `src-tauri/src/companion/dto.rs` for the canonical Rust types. The
serialized JSON shapes are stable and treated as the contract.

## Pairing

```
POST /v1/pair/start
→ 200
{
  "pairing_code": "381729",
  "qr_payload":   "{ \"version\": 1, \"mac_name\": \"...\", \"host\": \"192.168.1.40\", ... }",
  "expires_at_ms": 1716392400000,
  "host": "192.168.1.40",
  "port": 47812,
  "cert_fingerprint_sha256": "abcd..."
}
```

```
POST /v1/pair/complete
Content-Type: application/json
{
  "pairing_code": "381729",
  "device_name": "Anurup's iPhone",
  "device_type": "iphone",            // "iphone" | "watch" | "other"
  "public_key":  null,                // reserved
  "app_version": "0.1.0 (1)"
}
→ 200
{
  "device_id":    "dev_iphone_a1b2c3d4",
  "access_token": "<48 alphanumeric chars>",
  "mac_name":     "Anurup MacBook Pro",
  "permissions":  ["ask", "search", "status", "manual_capture", "capture_control", "feedback"]
}
```

Errors: `400 pairing_code_invalid` (unknown / expired), `409 pairing_code_used`
(slot already consumed), `400 bad_request` (empty `device_name`).

## Status

```
GET /v1/status
Authorization: Bearer <token>
→ 200
{
  "capture_status":     "running",        // running | paused | incognito
  "runtime_status":     "available",      // available | loading | unavailable
  "last_memory_at_ms":  1716392399000,    // or null
  "storage_status":     "healthy",
  "model_status":       "available",
  "active_project":     null,
  "mac_name":           "Anurup MacBook Pro",
  "app_version":        "0.2.11"
}
```

## Capture control

```
POST /v1/capture/control
Authorization: Bearer <token>
{
  "action":           "pause",      // pause | resume | incognito
  "duration_minutes": 15,           // optional; pause/incognito only
  "reason":           "mobile_user_request"
}
→ 200
{
  "capture_status": "paused",
  "is_paused":      true,
  "is_incognito":   false,
  "until":          "2026-05-22T12:00:00+00:00"   // or null
}
```

## Manual capture

```
POST /v1/memories/manual
Authorization: Bearer <token>
{
  "text":            "Remember to ship the companion API.",
  "client_event_id": "uuid-from-iphone",
  "capture_type":    "idea",        // idea | todo | decision | note | link | question
  "project":         "FNDR",
  "topic":           null,
  "source_override": null            // reserved; default derives from device type
}
→ 200
{
  "memory_id":   "<uuid v5 from (device_id, client_event_id)>",
  "status":      "indexed",
  "source_type": "iphone_manual_capture",
  "duplicate":   false
}
```

Provenance: `source_type` is forced from the authenticated device type
(`iphone_manual_capture` or `watch_manual_capture`). Idempotency: the
memory id is derived deterministically from `(device_id, client_event_id)`
— retrying the same capture from the iOS offline queue yields the same id,
and the Mac's content-hash dedup absorbs the duplicate silently.

## Ask FNDR

```json
POST /v1/ask
Authorization: Bearer <token>
{
  "query": "What was I working on today?",
  "limit": 8,
  "answer_style": "short"
}
```

```json
200
{
  "query": "What was I working on today?",
  "answer": "...",
  "verify_outcome": "grounded",
  "source_cards": [ { "memory_id": "...", "title": "...", "summary": "..." } ],
  "latency_ms": 42
}
```

## Memory search

```json
POST /v1/memories/search
Authorization: Bearer <token>
{
  "query": "fndr companion",
  "limit": 20,
  "time_filter": "today",
  "app_filter": "Xcode",
  "project_filter": "FNDR"
}
```

```json
200
{
  "query": "fndr companion",
  "cards": [ { "memory_id": "...", "title": "...", "summary": "..." } ],
  "total": 1,
  "latency_ms": 15
}
```

## Memory detail

```json
GET /v1/memories/:memory_id
Authorization: Bearer <token>
```

```json
200
{
  "card": { "memory_id": "...", "title": "...", "summary": "...", "internal_context": "..." }
}
```

## Feedback

```json
POST /v1/feedback
Authorization: Bearer <token>
{
  "event": "thumbs_up",
  "query": "what was I working on today",
  "memory_id": "..."
}
```

```json
200
{ "status": "ok" }
```

## Error envelope

Any non-2xx response uses this body:

```json
{ "error": "pairing_code_invalid", "message": "pairing code is invalid or expired" }
```

Stable `error` codes (used by mobile to decide UX, not parsed from `message`):

- `unauthenticated` (401)
- `forbidden` (403 — revoked / unknown token)
- `insufficient_permission` (403 — valid token, missing route scope)
- `pairing_code_invalid` (400)
- `pairing_code_used` (409)
- `bad_request` (400)
- `not_found` (404)
- `internal` (500)

## curl smoke pack

Tested end-to-end against `npm run tauri dev`. Substitute the
`HOST`, `PORT`, and `CODE` placeholders with the values from
`~/.fndr/companion.json` and the React Settings panel.

```bash
HOST=$(jq -r .host ~/.fndr/companion.json)
PORT=$(jq -r .port  ~/.fndr/companion.json)
BASE="https://$HOST:$PORT"

# 1. Liveness — should return {"ok": true}
curl -sk "$BASE/v1/health"

# 2. Pair start — print a code in the Mac UI, then run pair/complete
CODE="<the 6-digit code from the panel>"
TOKEN_JSON=$(curl -sk -X POST -H 'Content-Type: application/json' \
  -d "{\"pairing_code\":\"$CODE\",\"device_name\":\"curl test\",\"device_type\":\"iphone\"}" \
  "$BASE/v1/pair/complete")
TOKEN=$(echo "$TOKEN_JSON" | jq -r .access_token)
DEVICE_ID=$(echo "$TOKEN_JSON" | jq -r .device_id)

# 3. Status
curl -sk -H "Authorization: Bearer $TOKEN" "$BASE/v1/status"

# 4. Pause + resume capture
curl -sk -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"action":"pause"}' "$BASE/v1/capture/control"
curl -sk -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"action":"resume"}' "$BASE/v1/capture/control"

# 5. Manual capture
curl -sk -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"text":"hello from curl","client_event_id":"evt_smoke_1","capture_type":"idea"}' \
  "$BASE/v1/memories/manual"

# 6. Revocation smoke (from the Mac side via Tauri command, then this should 403):
#    pnpm tauri invoke companion_revoke_device --args "{\"deviceId\":\"$DEVICE_ID\"}"
curl -sk -H "Authorization: Bearer $TOKEN" "$BASE/v1/status"
# → {"error":"forbidden", ...}
```

All `curl` commands use `-k` because the cert is self-signed; iOS will
trust the cert via the fingerprint pinned at pairing time.
