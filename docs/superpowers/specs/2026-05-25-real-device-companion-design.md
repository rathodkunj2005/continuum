# Real-Device Companion: iPhone + Apple Watch Install & Pairing

**Date:** 2026-05-25
**Branch target:** `companion/slice-2-ios-shell` (merges into `main`)
**Status:** Approved — ready for implementation planning

## Goal

Install the FNDR iOS app and its paired Apple Watch app on physical devices.
Establish a working LAN pairing between those devices and the Mac Companion API.
Produce a repeatable runbook for re-deploying after the free-signing 7-day certificate expiry.

## Non-goals (explicitly deferred)

- Tauri autostart at Mac login — include only if `tauri-plugin-autostart` is already in `Cargo.toml`; otherwise defer.
- QR camera scanner (AVFoundation) — include only if trivial; paste/manual pairing is the required fallback.
- mDNS/Bonjour rediscovery of the Mac after an IP change.
- Slice 3+ iOS features (Ask, Memories detail, Capture offline queue feature work).
- Large WatchConnectivity additions — only bundle/signing linkage and install-path confirmation.
- The four currently-unmerged storage/telemetry files (`lance_store/arrow_and_filters.rs`, `lance_store/schemas.rs`, `storage/schema.rs`, `telemetry/system_metrics.rs`) — do not touch.
- TestFlight, App Store Connect, external beta.

---

## Context: what already exists on `companion/slice-2-ios-shell`

The iOS project at `apps/ios/` is fully scaffolded. Key facts verified by inspection:

- `apps/ios/FNDR.xcodeproj` — XcodeGen-generated.
- `apps/ios/project.yml` — source of truth; Watch target currently uses deprecated `type: application.watchapp2`.
- `apps/ios/FNDRKit/` — shared Swift package: `CompanionClient`, `PairingFlow`, `KeychainStore`, `ConnectionStatusService`, `OfflineCaptureQueue`, `WatchBridge`, `WatchBridgeClient`, full test suites.
- `apps/ios/FNDR/` — SwiftUI app: Pairing, Ask, Memories, Capture, Status, Settings, WatchBridge.
- `apps/ios/FNDR Watch/` — Watch app: Ask, Remember, Recent, Status, `WatchBridgeClient`.
- `PairingView.swift` uses a `TextEditor` paste-JSON flow — no camera scanner yet.
- `CompanionDevicesPanel.tsx` (desktop React) exists but is imported nowhere in the running app — the Mac Settings UI has no pairing panel visible to the user.
- The Mac Companion server binds to `127.0.0.1` by default. Physical devices on Wi-Fi cannot reach it.
- `.gitignore` has no iOS rules.

---

## Architecture overview

No new services, databases, or network protocols. Five discrete changes close the gap to a working real-device install:

| # | Area | Change |
|---|------|--------|
| A | Mac Rust | Opt-in mobile-pairing mode: bind `0.0.0.0`, advertise real LAN IP in QR payload |
| B | Mac TypeScript | Mount `CompanionDevicesPanel` into the running Settings UI; add diagnostic strip + toggle |
| C | iOS XcodeGen | Fix Watch target type; add `Local.xcconfig` signing override mechanism |
| D | iOS Swift | Pairing validation: reject loopback on physical device; probe connection before persisting token |
| E | Repo | iOS noise rules in `.gitignore`; `Local.xcconfig.example`; runbook |

The data flow for a successful real-device pairing after these changes:

```
Mac (tauri dev)
  └─ Companion server bound 0.0.0.0 on port P
  └─ Settings panel → diagnostic strip shows LAN IP 192.168.x.y
  └─ "Enable mobile pairing" toggle ON
  └─ "Generate pairing code" → QR JSON  {host: "192.168.x.y", mode: "lan", ...}

iPhone (same Wi-Fi)
  └─ PairingView: user pastes QR JSON
  └─ accept(): validate version, expiry, host not loopback, mode not "loopback_only"
  └─ complete(): POST /v1/pair/complete → receive token
  └─ probe: GET /v1/status with new token → 200 OK
  └─ persist: write token + PairedMac to Keychain
  └─ Status tab: live data within ~3 s

Apple Watch (paired to iPhone)
  └─ WCSession → PhoneWatchBridge → CompanionSession → Mac
  └─ No separate auth; iPhone is the only device that holds the token
```

---

## Section A — Mac Rust: companion server bind and advertise

### Files

| File | Change |
|------|--------|
| `src-tauri/src/config.rs` | Add three new fields to companion config |
| `src-tauri/src/companion/mod.rs` | `resolve_advertise_host()`, new bind/advertise params, `mode` in discovery file |
| `src-tauri/src/companion/dto.rs` | Add `mode: String` to `QrPayload` |
| `src-tauri/src/companion/pairing.rs` | Pass advertised host + mode into `QrPayload` |

### New config fields (`src-tauri/src/config.rs`)

```toml
[companion]
mobile_pairing_enabled = false          # default: safe loopback
bind_host = ""                          # override bind addr when mobile pairing ON; empty = "0.0.0.0"
advertise_host = ""                     # override advertised IP in QR; empty = auto-resolve
```

All three default to the safe/auto values when absent from the config file.

### Binding behavior

**mobile_pairing_enabled = false (default)**

The server binds to `127.0.0.1`. `advertise_host` is `"127.0.0.1"`. Discovery file and QR payload both record `mode = "loopback_only"`. No LAN exposure.

**mobile_pairing_enabled = true**

Bind host = `companion.bind_host` if set, else `"0.0.0.0"`.

`resolve_advertise_host()` runs:

1. If `companion.advertise_host` is set (non-empty) → return it verbatim.
2. Enumerate system network interfaces. For each, exclude:
   - Addresses in `127.0.0.0/8` (loopback)
   - Addresses in `169.254.0.0/16` (link-local / APIPA)
   - Addresses in `fe80::/10` (IPv6 link-local)
   - Interfaces whose OS name begins with `utun` or `ipsec` (VPN tunnels on macOS)
3. From the remaining IPv4 addresses, prefer interface names in this order: `en0`, `en1`, then alphabetical. Return the first match.
4. **If no usable address found:** return `Err(CompanionError::NoLanInterface)`. The server still starts on loopback. `companion.json` records `mode = "lan_unavailable"`. The Settings panel shows "Wi-Fi unavailable — connect to Wi-Fi before pairing." The "Generate pairing code" button is **disabled**; no QR is shown.

On success with a LAN IP: discovery file and QR payload record `mode = "lan"`.

### `mode` field (canonical values)

| Value | Meaning |
|-------|---------|
| `loopback_only` | Server on loopback. Not reachable by physical devices. |
| `lan` | Server reachable at the advertised LAN IP. |
| `lan_unavailable` | Mobile pairing requested but no usable LAN interface found. |

The `mode` field is added to `QrPayload` in `dto.rs` and serialised into the QR JSON. It is also written to `companion.json` for the Settings panel to read.

### Hot toggle behavior

Flipping `mobile_pairing_enabled` calls the existing `companion_stop_server` + `companion_start_server` Tauri commands with the new config. Already-paired devices are not revoked; their stored `host` becomes stale if they were paired on a different bind address, and will hit a connection error prompting a re-pair.

### New Tauri command

`companion_set_mobile_pairing(enabled: bool)` — persists the config toggle and restarts the server. Called from the Settings panel toggle.

### Existing command update

Add `mode: String` and `advertise_host: String` to the struct returned by `companion_get_endpoint` (or `companion_get_status` — implementer chooses the lowest-risk path that doesn't break existing callers). The Settings panel reads these fields to render the diagnostic strip.

### New Rust unit tests

- `advertise_host_picks_en0` — stub two interfaces (loopback + `en0/192.168.1.42`); assert `en0` IP returned.
- `advertise_host_excludes_link_local` — stub one `169.254.x.x` interface only; assert `Err(NoLanInterface)`.
- `advertise_host_excludes_utun` — stub `utun0` + loopback only; assert `Err(NoLanInterface)`.
- `qr_payload_serializes_mode_field` — round-trip serialize/deserialize `QrPayload` with `mode = "lan"`; assert field present and unchanged.

### Security posture

When `mobile_pairing_enabled = true`, the Companion API is reachable by any device on the same Wi-Fi network. Protections in place: TLS (self-signed cert, fingerprint-pinned by clients), bearer-token auth on all data routes, single-use 6-digit pairing code with a short TTL.

Operator guidance (documented in the runbook and the Settings UI): do not enable mobile pairing on untrusted networks (airports, hotels, corporate guest Wi-Fi). Disable after pairing is complete if preferred.

---

## Section B — Mac TypeScript: mount the pairing panel

### Problem

`src/domains/companion/CompanionDevicesPanel.tsx` is never imported. The running Mac app has no visible pairing UI.

### Change

Find the real Settings route container by grepping `src/` for the Settings page/route. Add a single import and `<CompanionDevicesPanel />` render inside it. Do not restructure the Settings page.

### Diagnostic strip (added inside `CompanionDevicesPanel.tsx`)

Above the pairing-code section, render a small strip using data from `companion_get_endpoint` (which now includes `mode` and `advertise_host`):

**Mode: loopback_only**
```
Bind: 127.0.0.1   Advertise: 127.0.0.1   Port: 47812   Mode: loopback only
[ Enable mobile pairing ]
```
Small note below: "Loopback only — simulator or Mac CLI pairing. Physical devices cannot connect."

**Mode: lan**
```
Bind: 0.0.0.0   Advertise: 192.168.1.42   Port: 47812   Mode: LAN
[ Disable mobile pairing ]
```
Amber notice: "FNDR is reachable by any device on your Wi-Fi. TLS + token-protected. Disable when not pairing."

**Mode: lan_unavailable**
```
Bind: 0.0.0.0   Advertise: —   Port: 47812   Mode: LAN (no Wi-Fi found)
[ Disable mobile pairing ]
```
Warning: "Connect to Wi-Fi, then toggle mobile pairing off and on to retry." Generate button disabled.

The "Enable/Disable mobile pairing" button calls `companion_set_mobile_pairing(enabled)` and re-polls after ~1 s.

### New Vitest test

`Settings route renders CompanionDevicesPanel` — renders the Settings route with mocked Tauri commands, asserts `CompanionDevicesPanel` is present in the output.

---

## Section C — iOS: `project.yml` and signing override

### Watch target type fix

The existing `type: application.watchapp2` causes duplicate build/package rules in Xcode 15+. Change to:

```yaml
FNDR Watch:
  type: application       # was: application.watchapp2
  platform: watchOS
  deploymentTarget: '10.0'
```

The following Info.plist keys must remain in the Watch target settings block:

```yaml
INFOPLIST_KEY_WKApplication: YES
INFOPLIST_KEY_WKCompanionAppBundleIdentifier: $(BUNDLE_ID_PREFIX).ios
```

### Signing override via `Local.xcconfig`

`apps/ios/Local.xcconfig` is **gitignored and never committed**. It is generated locally by `make ios-bootstrap`.

**Committed reference file** — `apps/ios/Local.xcconfig.example`:
```xcconfig
// Copy this file to Local.xcconfig and fill in your values.
// Local.xcconfig is gitignored — never commit the real file.
DEVELOPMENT_TEAM =
BUNDLE_ID_PREFIX =
```

**`project.yml` changes:**
- Add `configFiles: { Debug: Local.xcconfig, Release: Local.xcconfig }` to both targets.
- Replace hardcoded `com.fndr.ios` with `$(BUNDLE_ID_PREFIX).ios` for `PRODUCT_BUNDLE_IDENTIFIER` in the iPhone target.
- Watch bundle ID: `$(BUNDLE_ID_PREFIX).ios.watchkitapp`.
- `WKCompanionAppBundleIdentifier`: `$(BUNDLE_ID_PREFIX).ios`.
- Remove `DEVELOPMENT_TEAM: ""` hardcoding from both targets (value comes from xcconfig).
- `CODE_SIGN_STYLE: Automatic` stays.

After `xcodegen generate`, Xcode's automatic signing will read `DEVELOPMENT_TEAM` from the xcconfig and allocate provisioning profiles for both bundle IDs under the personal team.

### `make ios-bootstrap` target

Added to the repo `Makefile`. Idempotent — running it again overwrites `Local.xcconfig` and regenerates the Xcode project.

```makefile
ios-bootstrap:
	@echo "=== FNDR iOS local signing setup ==="
	@read -p "Apple Developer Team ID (10 chars, from developer.apple.com/account): " team; \
	 read -p "Bundle ID prefix (e.g. com.yourname): " prefix; \
	 printf "DEVELOPMENT_TEAM = %s\nBUNDLE_ID_PREFIX = %s\n" "$$team" "$$prefix" \
	   > apps/ios/Local.xcconfig; \
	 echo "Written apps/ios/Local.xcconfig (gitignored)."
	xcodegen generate --spec apps/ios/project.yml --project apps/ios
	@echo "Done. Open apps/ios/FNDR.xcodeproj in Xcode."
```

`xcodegen` must be installed (`brew install xcodegen`). The `make` target does not install it; the runbook mentions the prerequisite.

---

## Section D — iOS Swift: pairing validation and connection probe

### `PairingFlow.accept(payload:)` — new validation rules

Run in order; first failure returns immediately. Rules 4–6 are new:

1. `payload.version == 1` — else "Unsupported QR payload version (\(version))."
2. Pairing code is exactly 6 numeric digits.
3. `payload.expiresAtMs > now()`.
4. `payload.host` is non-empty.
5. **Physical device only** (`#if !targetEnvironment(simulator)`): if `payload.host` is `"127.0.0.1"`, `"::1"`, or `"localhost"` → fail with `"This pairing code is for simulator only — enable mobile pairing in FNDR Mac Settings, then paste a new code."` The simulator path continues to accept loopback so existing simulator smoke works unchanged.
6. **Any environment:** if `payload.mode == "loopback_only"` → same message as rule 5. This catches any future case where the host string is non-loopback but the server's own mode field says it isn't LAN-reachable.

### `PairingFlow.complete(...)` — connection probe before persist

**Current behavior:** POST `/v1/pair/complete` → on 200, immediately write keychain → `.paired`.

**New behavior:**
1. POST `/v1/pair/complete` → receive `access_token`, `device_id`, `mac_name`, `permissions`.
2. Build a temporary `CompanionClient` configured with the new token and the QR payload's `host`/`port`.
3. Call `GET /v1/status`. If the call returns any error or a non-200 response:
   - **Do not write to the keychain.**
   - Set state `.failed("Pairing token issued but the Mac didn't respond at \(host):\(port). Are both devices on the same Wi-Fi?")`.
   - The orphaned token on the Mac is inert; the user re-pairs with a fresh code, or revokes from Mac Settings.
4. On 200 from the probe → write keychain entries → `.paired`.

The existing two-call keychain write sequence is otherwise unchanged.

### `ConnectionStatusService` — re-pair affordance

No new screens needed. In the existing iOS Settings tab, when the service is in the `.unreachable` / `.disconnected` state, surface a "Re-pair this Mac" button. Tapping it:
- Clears Keychain entries (`accessToken`, `pairedMac`).
- Resets `CompanionSession` to unpaired state.
- Navigates to the pairing screen.

This handles: IP changed, Wi-Fi changed, TLS cert regenerated on the Mac.

If `CompanionSession` already has an `unpair()` method, use it. Otherwise add one.

### New Swift tests

- `pairing_rejects_loopback_on_device` — use a compile-time flag or test-only override to simulate non-simulator environment; inject loopback host; assert `.failed` state with the "simulator only" message.
- `pairing_rejects_loopback_only_mode_string` — inject `mode = "loopback_only"` with a valid non-loopback host; assert `.failed` on any environment.
- `pairing_status_probe_failure_does_not_persist` — mock transport: step 1 returns 200 pair response, step 2 returns 503; assert keychain has no entries and state is `.failed`.

---

## Section E — `.gitignore` additions and gitignore hygiene

Append the following section to `.gitignore`. No existing rules are removed or reordered.

```gitignore
# iOS / Xcode
apps/ios/Local.xcconfig
apps/ios/DerivedData/
apps/ios/build/
apps/ios/*.xcresult
apps/ios/**/*.xcresult
apps/ios/.swiftpm/
xcuserdata/
*.xcuserstate
*.xcworkspace/xcuserdata/
*.xcodeproj/xcuserdata/
*.xcodeproj/project.xcworkspace/xcuserdata/

# Fastlane (pre-emptive, if added later)
fastlane/report.xml
fastlane/Preview.html
fastlane/screenshots/
fastlane/test_output/
```

Before committing, the implementer must run:

```bash
git status --short
git diff --stat
git diff -- .gitignore
git ls-files | grep -E 'xcuserdata|DerivedData|xcuserstate|xcresult|/build/|\.swiftpm'
```

If any tracked local/generated files appear in that last command, remove them from the index only (not from disk):

```bash
git rm --cached <path>
```

Files that must remain tracked: `apps/ios/project.yml`, `apps/ios/FNDR.xcodeproj/project.pbxproj`, all Swift source files, `Local.xcconfig.example`, `Package.swift`, any intentional `Info.plist` or entitlements files.

---

## Section F — Runbook (`docs/companion/real-device-runbook.md`)

The runbook is committed alongside the code changes and is a first-class deliverable.

### 1. Prerequisites

- Xcode full app installed (not Command Line Tools only). Confirm: `xcodebuild -version`.
- `xcodegen` installed: `brew install xcodegen`.
- Apple ID (free personal team is sufficient). Team ID visible at developer.apple.com → Account → Membership.
- Apple Watch paired to the target iPhone and running watchOS 10+.
- Mac and iPhone on the same home Wi-Fi network, no client isolation between devices.
- FNDR repo cloned; on branch `companion/slice-2-ios-shell` or a branch containing these changes.

### 2. One-time Mac setup

```bash
make ios-bootstrap
# Prompts for Team ID and bundle prefix
# Writes apps/ios/Local.xcconfig (gitignored)
# Runs xcodegen to regenerate apps/ios/FNDR.xcodeproj
```

Verify no warnings about duplicate targets or missing files during `xcodegen generate`.

### 3. Start the Mac runtime

```bash
npm run tauri dev
```

Open FNDR → Settings → Paired Devices (now visible after Section B changes).

Verify the diagnostic strip:
- With mobile pairing OFF: shows `Mode: loopback only`. This is expected.
- Click **Enable mobile pairing**. Strip updates to `Mode: LAN`, advertised host is a `192.168.x.y` address (not `127.0.0.1`). If it shows "Wi-Fi unavailable," ensure the Mac is connected to Wi-Fi, then toggle mobile pairing off and back on.

Click **Generate pairing code**. Copy the full QR JSON payload text.

### 4. iPhone first install

1. Plug the iPhone into the Mac via USB.
2. Open `apps/ios/FNDR.xcodeproj` in Xcode.
3. Select the `FNDR` scheme and the physical iPhone as the destination.
4. Click **Run** (▶). Xcode will register the device and request a provisioning profile automatically.
5. On the iPhone: iOS will prompt **"Untrusted Developer."** Go to **Settings → VPN & Device Management → Developer App → [your Apple ID] → Trust**.
6. Re-run from Xcode. The app launches.

### 5. Pair from iPhone

1. In the FNDR app, tap **Pair** (or navigate to the pairing screen).
2. Paste the QR JSON payload copied from step 3.
3. Tap **Validate payload**. Confirm no error appears. If you see "This pairing code is for simulator only," go back to the Mac, verify mobile pairing is ON and the advertised host is a LAN IP, then regenerate the code.
4. Tap **Complete pairing**. The app probes `/v1/status` with the new token. On success, the Status tab populates within a few seconds.

### 6. Watch first install

The iPhone app must be installed (step 4) before the Watch app can install.

1. In Xcode, change the scheme to **FNDR Watch**.
2. Set the destination to the paired Apple Watch.
3. Click **Run**. Xcode installs the Watch app directly when the Watch is connected via the paired iPhone.
4. If the Watch app does not appear on the Watch face after install: open the **Watch app on iPhone** → **My Watch** tab → scroll down → find **FNDR** → tap **Install**.

### 7. Smoke checklist

Run these after pairing is confirmed. Record pass/fail in the PR description.

**iPhone**
- [ ] Status tab: capture status, runtime status, storage status fields all populated.
- [ ] Capture tab: submit a manual "remember" note → note appears in Mac vault.
- [ ] Disable Wi-Fi on iPhone → Status tab shows "unreachable" or similar error state.
- [ ] Re-enable Wi-Fi → next Status refresh succeeds.
- [ ] iOS Settings → "Re-pair this Mac" button → clears state, returns to pairing screen.

**Watch**
- [ ] Open FNDR on Watch → Status screen shows data (via WCSession → iPhone → Mac).
- [ ] Watch Remember screen: dictate or type a note → it lands in the Mac vault.

**Mac diagnostics**
- [ ] Mobile pairing OFF → diagnostic strip shows "loopback only," generate button produces a loopback QR.
- [ ] Mobile pairing ON → toggle it OFF → strip reverts. Toggle ON again → LAN IP returns.

### 8. Failure-mode index

| Symptom | Cause | Fix |
|---------|-------|-----|
| "Untrusted Developer" on iPhone launch | Free-signing cert not trusted yet | Settings → VPN & Device Management → Developer App → Trust |
| "Unable to Verify App" on launch | Free-signing 7-day cert expired | Re-deploy from Xcode (select device, Run). No re-pairing needed unless token was revoked. |
| Pairing rejected: "simulator only" | QR host is loopback or mode is loopback_only | Enable mobile pairing on Mac, verify LAN IP in diagnostic strip, regenerate code |
| "Pairing token issued but the Mac didn't respond" | Connection probe failed — different Wi-Fi or firewall | Ensure Mac and iPhone are on the same Wi-Fi network and the Mac has no firewall blocking the companion port |
| Watch app not on Watch after install | iPhone app not installed first, or Watch not paired | Install iPhone target first; check Watch app on iPhone → My Watch → FNDR → Install |
| Status tab stays "unreachable" after Wi-Fi restored | Mac's IP changed (DHCP renewed) | Tap "Re-pair this Mac" on iPhone, generate a new code on the Mac, re-pair |
| TLS fingerprint mismatch error | Mac's self-signed cert was regenerated | Tap "Re-pair this Mac," re-pair with the new QR which carries the updated fingerprint |
| Diagnostic strip shows wrong LAN IP | Auto-resolver picked a non-Wi-Fi interface | Set `companion.advertise_host = "192.168.x.y"` explicitly in FNDR config to override |
| `xcodegen generate` warns about duplicate targets | Using old `application.watchapp2` target type | Verify `project.yml` Watch target uses `type: application` (Section C fix) |

### 9. Weekly re-deploy (free-signing expiry)

Free-signing certificates expire after 7 days. When the iPhone shows "Unable to Verify App":

1. Plug iPhone into Mac.
2. Open Xcode, select `FNDR` scheme + iPhone destination.
3. Run (▶). Xcode refreshes the certificate automatically.
4. Trust the app again in iOS Settings if prompted.
5. Repeat for `FNDR Watch` scheme if the Watch app also expired.

No re-pairing is required (the Keychain token survives re-deployment) unless you also revoked the device from Mac Settings.

---

## Section G — Acceptance criteria

This slice is done when all of the following hold.

### Tests

- [ ] `cargo test -p src-tauri` passes, including 4 new companion Rust unit tests.
- [ ] `swift run FNDRKitCheck` (or `swift test` in `apps/ios/FNDRKit/`) passes, including 3 new Swift cases.
- [ ] `pnpm vitest` passes, including the new Settings-route mount test.
- [ ] The 4 unmerged storage/telemetry files remain untouched — verify with `git diff --stat` showing no changes to `lance_store/` or `telemetry/`.

### Gitignore hygiene

- [ ] `git ls-files | grep -E 'xcuserdata|DerivedData|xcuserstate|xcresult|/build/|\.swiftpm'` returns no output.
- [ ] `apps/ios/Local.xcconfig` does not appear in `git ls-files`.
- [ ] `apps/ios/Local.xcconfig.example` appears in `git ls-files`.

### Real-device smoke (run by you, recorded in the PR description)

- [ ] `make ios-bootstrap` + `xcodegen generate` completes without duplicate-target warnings.
- [ ] iPhone app installs and launches from Xcode with personal team.
- [ ] Mac Settings panel renders `CompanionDevicesPanel`; mobile-pairing toggle works; diagnostic strip updates correctly.
- [ ] After enabling mobile pairing, QR JSON shows `host = <LAN IP>` and `mode = "lan"`.
- [ ] iPhone pairing succeeds; Status tab shows live Mac data.
- [ ] Manual capture from iPhone lands in Mac vault.
- [ ] Watch app installs to physical Watch.
- [ ] Watch Status and Remember screens work via WCSession.
- [ ] Loopback QR pasted into physical iPhone is rejected with the correct error message.
- [ ] Mobile pairing OFF: generate button disabled or payload labeled loopback-only.
- [ ] Mobile pairing ON + Mac not on Wi-Fi: panel shows "Wi-Fi unavailable."

### Deliverables in the commit set

- [ ] `docs/companion/real-device-runbook.md` committed and matches the sequence actually performed.
- [ ] `apps/ios/Local.xcconfig.example` committed.
- [ ] `apps/ios/Local.xcconfig` gitignored and absent from the index.
- [ ] `.gitignore` contains the iOS section from Section E.
- [ ] `apps/ios/project.yml` Watch target uses `type: application`.
- [ ] `companion_set_mobile_pairing` Tauri command exists and is invocable.
- [ ] `CompanionDevicesPanel` is reachable from the running Mac app.

---

## Open decisions for the implementer

- **Settings mount point** — grep `src/` for the Settings route container before editing. The spec names `CompanionDevicesPanel` but does not pin the exact parent file path; the implementer must confirm what actually exists.
- **`companion_get_endpoint` vs new struct field** — cheapest path is to add `mode: String` and `advertise_host: String` to the existing `CompanionEndpointInfo` returned by `companion_get_endpoint`. Confirm this doesn't break existing callers before adding a new command.
- **QR camera scanner (optional)** — if time and risk allow, add `apps/ios/FNDR/Pairing/QRScannerView.swift` (~80 LOC `UIViewControllerRepresentable` wrapping `AVCaptureMetadataOutput`) and a toggle in `PairingView` between scan mode and paste mode. Add `NSCameraUsageDescription` to `project.yml`. If it adds risk or complexity, defer to the next slice; paste remains the documented path.
- **Tauri autostart (optional)** — if `tauri-plugin-autostart` is already present in `Cargo.toml`, wire a `companion_set_autostart(enabled: bool)` command and add a toggle to the Settings panel. If not present, do not add the dependency in this slice.
- **`local-ip-address` crate** — if not already in `src-tauri/Cargo.toml`, add it. Verify the license is compatible (MIT). If adding a new dep proves problematic, implement manual interface enumeration via `std::net` + `nix` syscalls (already a transitive dep of Tauri) as the fallback.
