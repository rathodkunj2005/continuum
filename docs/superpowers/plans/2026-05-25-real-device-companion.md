# Real-Device Companion: iPhone + Apple Watch Install & Pairing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Install and pair the FNDR iOS + Watch app on a physical iPhone and its paired Apple Watch, connected to the Mac Companion API over LAN Wi-Fi, with a repeatable runbook.

**Architecture:** The Mac Companion server (Axum/Rust, TLS) gains a `mobile_pairing_enabled` toggle (default `true` — LAN binding already active on this branch) and a `mode` field that flows through the QR payload to iOS. iOS `PairingFlow` validates the mode/host before accepting a pairing and probes `/v1/status` with the new token before writing to the Keychain. The ControlPanel in the desktop React app gains a "Paired Devices" tab that surfaces the diagnostic strip and toggle. The iOS Xcode project is updated so any developer can supply their own signing identity via a gitignored `Local.xcconfig`.

**Tech Stack:** Rust (Axum, Tokio, serde_json), TypeScript + React (Tauri IPC), Swift 5.10 (SwiftUI, URLSession, WatchConnectivity, Keychain), XcodeGen, GNU Make.

**Branch:** `companion/slice-2-ios-shell`

**DO NOT TOUCH:** `src-tauri/src/storage/lance_store/arrow_and_filters.rs`, `src-tauri/src/storage/lance_store/schemas.rs`, `src-tauri/src/storage/schema.rs`, `src-tauri/src/telemetry/system_metrics.rs` — these have separate in-progress history and are out of scope.

**Key fact about the current codebase:**
The companion server on this branch already binds to `0.0.0.0` by default (`LAN_BIND_HOST` in `src-tauri/src/companion/mod.rs`). The `resolve_advertised_host()` function already exists using a UDP-socket-routing trick. `mobile_pairing_enabled` defaults to `true` in this plan (matching existing behavior). The spec doc at `docs/superpowers/specs/2026-05-25-real-device-companion-design.md` says `false`; that was written before the slice-2 code was inspected — treat the plan as authoritative.

---

## File Map

| File | Create / Modify | Purpose |
|------|----------------|---------|
| `src-tauri/src/companion/mod.rs` | Modify | Add `bind_mode`/`mobile_pairing_enabled` to runtime; `resolve_advertised_host_and_mode()`; pass mode to discovery + endpoint |
| `src-tauri/src/companion/dto.rs` | Modify | Add `mode: String` to `CompanionEndpoint`; add `mode: String` + `advertise_host: String` to `CompanionStatusPayload` in commands |
| `src-tauri/src/companion/pairing.rs` | Modify | Add `mode: String` to `QrPayload`; add `mode: String` to `PairingEndpointHint` |
| `src-tauri/src/ipc/commands/companion.rs` | Modify | Add `companion_set_mobile_pairing` command; add `mode`/`advertise_host` to `CompanionStatusPayload`; register new command |
| `src-tauri/src/lib.rs` | Modify | Register `companion_set_mobile_pairing` in Tauri command list |
| `src/shared/ipc/tauri.ts` | Modify | Add `mode`/`advertise_host` to `CompanionStatusPayload`; add `companionSetMobilePairing` binding |
| `src/domains/companion/CompanionDevicesPanel.tsx` | Modify | Add diagnostic strip + mobile-pairing toggle |
| `src/domains/companion/__tests__/CompanionDevicesPanel.test.tsx` | Modify | Add test for mode diagnostic strip |
| `src/domains/workspace/ControlPanel.tsx` | Modify | Add `"companion"` tab; import + render `CompanionDevicesPanel` |
| `apps/ios/FNDRKit/Sources/FNDRKit/DTOs.swift` | Modify | Add `mode: String?` to `QRPayload` |
| `apps/ios/FNDRKit/Sources/FNDRKit/PairingFlow.swift` | Modify | Add `isSimulator: Bool` param; loopback + mode validation; connection probe |
| `apps/ios/FNDRKit/Sources/FNDRKitCheck/PairingFlowSuite.swift` | Modify | Add 3 new test cases |
| `apps/ios/FNDRKit/Tests/FNDRKitTests/PairingFlowTests.swift` | Modify | Add same 3 cases as XCTest |
| `apps/ios/project.yml` | Modify | Fix Watch target type; add `configFiles`; derive bundle IDs from `BUNDLE_ID_PREFIX` |
| `apps/ios/Local.xcconfig.example` | Create | Committed template; the real `Local.xcconfig` is gitignored |
| `Makefile` | Modify | Add `ios-bootstrap` target |
| `.gitignore` | Modify | Add `apps/ios/Local.xcconfig` and any missing iOS rules |
| `docs/companion/real-device-runbook.md` | Create | End-to-end runbook for personal-device install + pairing |

---

## Task 1: Gitignore hygiene

**Files:**
- Modify: `.gitignore`

- [ ] **Step 1: Check what iOS rules are already present**

```bash
grep -n "xcuserdata\|DerivedData\|xcuserstate\|xcresult\|Local.xcconfig\|swiftpm\|fastlane" .gitignore
```

Expected: you'll see some rules (the system-reminder showed `.build/`, `DerivedData/`, `xcuserdata/`). Note what's already there.

- [ ] **Step 2: Add any missing rules**

Open `.gitignore`. Ensure the following are present (add only the missing lines, don't duplicate):

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

# Fastlane (pre-emptive)
fastlane/report.xml
fastlane/Preview.html
fastlane/screenshots/
fastlane/test_output/
```

- [ ] **Step 3: Verify no tracked noise files exist**

```bash
git ls-files | grep -E 'xcuserdata|DerivedData|xcuserstate|xcresult|Local\.xcconfig|\.swiftpm'
```

Expected: no output. If any files appear, run `git rm --cached <path>` for each.

- [ ] **Step 4: Commit**

```bash
git add .gitignore
git commit -m "chore: add missing iOS/Xcode gitignore rules"
```

---

## Task 2: Mac Rust — `mode` field through the Companion API

**Files:**
- Modify: `src-tauri/src/companion/pairing.rs`
- Modify: `src-tauri/src/companion/mod.rs`
- Modify: `src-tauri/src/companion/dto.rs`

### 2a — Add `mode` to `QrPayload` and `PairingEndpointHint`

- [ ] **Step 1: Write a failing test in `pairing.rs`**

Add this inside the existing `#[cfg(test)]` block in `src-tauri/src/companion/pairing.rs`:

```rust
#[test]
fn qr_payload_includes_mode_field() {
    let qr = QrPayload {
        version: 1,
        mac_name: "Test Mac".to_string(),
        host: "192.168.1.42".to_string(),
        port: 47812,
        tls: true,
        cert_fingerprint_sha256: None,
        pairing_code: "123456".to_string(),
        expires_at_ms: 9_999_999_999_999,
        mode: "lan".to_string(),
    };
    let json = serde_json::to_string(&qr).unwrap();
    assert!(json.contains("\"mode\":\"lan\""), "mode field missing from QrPayload JSON: {json}");
}
```

- [ ] **Step 2: Run — expect FAIL (mode field not on struct)**

```bash
cd src-tauri && cargo test companion::pairing::tests::qr_payload_includes_mode_field 2>&1 | tail -20
```

Expected: compile error — `mode` not a field of `QrPayload`.

- [ ] **Step 3: Add `mode` to `QrPayload` and `PairingEndpointHint`**

In `src-tauri/src/companion/pairing.rs`, modify the two structs:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct QrPayload {
    pub version: u32,
    pub mac_name: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub cert_fingerprint_sha256: Option<String>,
    pub pairing_code: String,
    pub expires_at_ms: i64,
    pub mode: String,          // ← new: "lan" | "loopback_only" | "lan_unavailable"
}

#[derive(Clone)]
pub struct PairingEndpointHint {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub mac_name: String,
    pub cert_fingerprint_sha256: Option<String>,
    pub mode: String,          // ← new
}
```

In `PairingService::start()` (the `start` method that builds `QrPayload`, around line 100 of pairing.rs), add the `mode` field from the hint:

```rust
let qr = QrPayload {
    version: 1,
    mac_name: hint.mac_name.clone(),
    host: hint.host.clone(),
    port: hint.port,
    tls: hint.tls,
    cert_fingerprint_sha256: hint.cert_fingerprint_sha256.clone(),
    pairing_code: code.clone(),
    expires_at_ms: expires_at_ms,
    mode: hint.mode.clone(),   // ← new
};
```

- [ ] **Step 4: Run test — expect PASS**

```bash
cd src-tauri && cargo test companion::pairing::tests::qr_payload_includes_mode_field 2>&1 | tail -10
```

Expected: `test companion::pairing::tests::qr_payload_includes_mode_field ... ok`

### 2b — `resolve_advertised_host_and_mode` + bind_mode in runtime

- [ ] **Step 5: Write failing tests in `mod.rs`**

Add inside a new `#[cfg(test)]` block at the bottom of `src-tauri/src/companion/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_lan_returns_lan_mode() {
        // When a real LAN IP is detected, mode must be "lan".
        // This test runs on a machine with Wi-Fi; skip gracefully if not.
        let (host, mode) = resolve_advertised_host_and_mode(LAN_BIND_HOST, true);
        if host == LOOPBACK_HOST {
            // No LAN — mode should be lan_unavailable, not loopback_only.
            assert_eq!(mode, "lan_unavailable");
        } else {
            assert_eq!(mode, "lan");
            assert_ne!(host, LOOPBACK_HOST);
        }
    }

    #[test]
    fn resolve_loopback_bind_returns_loopback_only_mode() {
        let (host, mode) = resolve_advertised_host_and_mode(LOOPBACK_HOST, false);
        assert_eq!(host, LOOPBACK_HOST);
        assert_eq!(mode, "loopback_only");
    }

    #[test]
    fn companion_endpoint_dto_includes_mode() {
        let ep = CompanionEndpoint {
            host: "192.168.1.1".to_string(),
            port: 9000,
            base_url: "https://192.168.1.1:9000".to_string(),
            tls: true,
            cert_fingerprint_sha256: None,
            mac_name: "Test".to_string(),
            app_version: "0.1.0".to_string(),
            mode: "lan".to_string(),
            advertise_host: "192.168.1.1".to_string(),
        };
        let json = serde_json::to_string(&ep).unwrap();
        assert!(json.contains("\"mode\":\"lan\""), "mode missing: {json}");
        assert!(json.contains("\"advertise_host\""), "advertise_host missing: {json}");
    }
}
```

- [ ] **Step 6: Run — expect FAIL**

```bash
cd src-tauri && cargo test companion::tests 2>&1 | tail -20
```

Expected: compile errors — `resolve_advertised_host_and_mode` and new `CompanionEndpoint` fields don't exist yet.

- [ ] **Step 7: Replace `resolve_advertised_host` with `resolve_advertised_host_and_mode` in `mod.rs`**

Replace the current `resolve_advertised_host` function (and its helpers) with:

```rust
/// Returns (advertised_host, mode).
/// mode is "lan" | "loopback_only" | "lan_unavailable".
fn resolve_advertised_host_and_mode(bind_host: &str, mobile_pairing_enabled: bool) -> (String, &'static str) {
    if !mobile_pairing_enabled {
        return (LOOPBACK_HOST.to_string(), "loopback_only");
    }
    // mobile_pairing_enabled = true: try to detect LAN IP.
    if !is_unspecified_host(bind_host) {
        // Explicit bind address — use as-is; mode depends on whether it's loopback.
        let mode = if bind_host == LOOPBACK_HOST { "loopback_only" } else { "lan" };
        return (bind_host.to_string(), mode);
    }
    match detect_primary_lan_ipv4() {
        Some(ip) => (ip.to_string(), "lan"),
        None => (LOOPBACK_HOST.to_string(), "lan_unavailable"),
    }
}
```

Keep `is_unspecified_host` and `detect_primary_lan_ipv4` unchanged.

- [ ] **Step 8: Add `bind_mode` and `mobile_pairing_enabled` to `CompanionRuntime`**

In the `CompanionRuntime` struct:

```rust
struct CompanionRuntime {
    running: bool,
    host: String,
    port: u16,
    tls: bool,
    base_url: String,
    mac_name: String,
    last_error: Option<String>,
    bind_mode: String,              // ← new: "lan" | "loopback_only" | "lan_unavailable"
    mobile_pairing_enabled: bool,   // ← new: persisted across stop/start
    task: Option<JoinHandle<()>>,
    server_handle: Option<axum_server::Handle>,
    pairing: Option<Arc<PairingService>>,
    registry: Option<Arc<DeviceRegistry>>,
}

impl Default for CompanionRuntime {
    fn default() -> Self {
        Self {
            running: false,
            host: String::new(),
            port: 0,
            tls: true,
            base_url: String::new(),
            mac_name: String::new(),
            last_error: None,
            bind_mode: "loopback_only".to_string(),
            mobile_pairing_enabled: true,   // default ON — matches existing 0.0.0.0 behavior
            task: None,
            server_handle: None,
            pairing: None,
            registry: None,
        }
    }
}
```

- [ ] **Step 9: Update `start()` to use `resolve_advertised_host_and_mode`**

In the `start()` function, replace the old `resolve_advertised_host` call:

```rust
// At the top of start(), read mobile_pairing_enabled from runtime before any await:
let mobile_pairing_enabled = {
    let rt = runtime().lock();
    rt.mobile_pairing_enabled
};

let bind_host = host.unwrap_or_else(|| {
    if mobile_pairing_enabled { LAN_BIND_HOST.to_string() } else { LOOPBACK_HOST.to_string() }
});
let (advertised_host, bind_mode) = resolve_advertised_host_and_mode(&bind_host, mobile_pairing_enabled);
```

Remove the old `let advertised_host = resolve_advertised_host(&bind_host);` line.

After computing `advertised_host` and `bind_mode`, update the `PairingEndpointHint` construction:

```rust
let endpoint_hint = PairingEndpointHint {
    host: advertised_host.clone(),
    port: actual_port,
    tls: true,
    mac_name: mac_name.clone(),
    cert_fingerprint_sha256: tls_cert_fingerprint(),
    mode: bind_mode.to_string(),   // ← new
};
```

In the runtime lock section after the server spawns, store `bind_mode`:

```rust
rt.bind_mode = bind_mode.to_string();
```

Update `write_discovery` call to pass mode:

```rust
write_discovery(&advertised_host, actual_port, true, &mac_name, bind_mode);
```

Update `write_discovery` signature:

```rust
fn write_discovery(host: &str, port: u16, tls_enabled: bool, mac_name: &str, mode: &str) {
    // ... existing path/mkdir code ...
    let payload = serde_json::json!({
        "host": host,
        "port": port,
        "tls": tls_enabled,
        "mode": mode,           // ← new
        "base_url": format!("{}://{}:{}", scheme, host, port),
        "cert_fingerprint_sha256": tls_cert_fingerprint(),
        "mac_name": mac_name,
        "app_version": env!("CARGO_PKG_VERSION"),
    });
    // ... existing write code ...
}
```

- [ ] **Step 10: Add `mode` and `advertise_host` to `CompanionEndpoint` in `dto.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionEndpoint {
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub tls: bool,
    pub cert_fingerprint_sha256: Option<String>,
    pub mac_name: String,
    pub app_version: String,
    pub mode: String,              // ← new
    pub advertise_host: String,    // ← new (same as host when LAN; useful for diagnostic UI)
}
```

- [ ] **Step 11: Update `endpoint()` in `mod.rs` to fill the new fields**

```rust
pub fn endpoint() -> Option<CompanionEndpoint> {
    let rt = runtime().lock();
    if !rt.running {
        return None;
    }
    Some(CompanionEndpoint {
        host: rt.host.clone(),
        port: rt.port,
        base_url: rt.base_url.clone(),
        tls: rt.tls,
        cert_fingerprint_sha256: tls_cert_fingerprint(),
        mac_name: rt.mac_name.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        mode: rt.bind_mode.clone(),         // ← new
        advertise_host: rt.host.clone(),    // ← new
    })
}
```

- [ ] **Step 12: Run tests — expect PASS**

```bash
cd src-tauri && cargo test companion 2>&1 | tail -20
```

Expected: all companion tests pass including the 3 new ones.

- [ ] **Step 13: Commit**

```bash
git add src-tauri/src/companion/mod.rs src-tauri/src/companion/pairing.rs src-tauri/src/companion/dto.rs
git commit -m "feat(companion): add mode field and resolve_advertised_host_and_mode"
```

---

## Task 3: Mac Rust — `companion_set_mobile_pairing` Tauri command

**Files:**
- Modify: `src-tauri/src/ipc/commands/companion.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/shared/ipc/tauri.ts`

- [ ] **Step 1: Write a failing test**

Add in `src-tauri/src/ipc/commands/companion.rs` (in `#[cfg(test)]` at bottom):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_status_payload_has_mode_field() {
        let payload = CompanionStatusPayload {
            running: true,
            host: "192.168.1.1".to_string(),
            port: 9000,
            tls: true,
            base_url: "https://192.168.1.1:9000".to_string(),
            mac_name: "Test".to_string(),
            last_error: None,
            mode: "lan".to_string(),
            advertise_host: "192.168.1.1".to_string(),
            mobile_pairing_enabled: true,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"mode\":\"lan\""));
        assert!(json.contains("\"mobile_pairing_enabled\":true"));
    }
}
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd src-tauri && cargo test companion::commands::tests 2>&1 | tail -10
```

Expected: compile error — fields don't exist on `CompanionStatusPayload`.

- [ ] **Step 3: Add fields to `CompanionStatusPayload` and implement the new command**

In `src-tauri/src/ipc/commands/companion.rs`, update the struct:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct CompanionStatusPayload {
    pub running: bool,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub base_url: String,
    pub mac_name: String,
    pub last_error: Option<String>,
    pub mode: String,                   // ← new
    pub advertise_host: String,         // ← new
    pub mobile_pairing_enabled: bool,   // ← new
}
```

Update `to_payload(s: &CompanionStatus)` helper (or inline where status is constructed) to fill those fields. First update `CompanionStatus` in `mod.rs` to carry them:

In `src-tauri/src/companion/mod.rs`, update `CompanionStatus`:

```rust
#[derive(Debug, Clone)]
pub struct CompanionStatus {
    pub running: bool,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub base_url: String,
    pub mac_name: String,
    pub last_error: Option<String>,
    pub mode: String,                 // ← new
    pub advertise_host: String,       // ← new
    pub mobile_pairing_enabled: bool, // ← new
}
```

Update `to_status()`:

```rust
fn to_status(rt: &CompanionRuntime) -> CompanionStatus {
    CompanionStatus {
        running: rt.running,
        host: rt.host.clone(),
        port: rt.port,
        tls: rt.tls,
        base_url: rt.base_url.clone(),
        mac_name: rt.mac_name.clone(),
        last_error: rt.last_error.clone(),
        mode: rt.bind_mode.clone(),
        advertise_host: rt.host.clone(),
        mobile_pairing_enabled: rt.mobile_pairing_enabled,
    }
}
```

In `commands/companion.rs`, update every `CompanionStatusPayload { ... }` construction to include the new fields:

```rust
CompanionStatusPayload {
    running: s.running,
    host: s.host,
    port: s.port,
    tls: s.tls,
    base_url: s.base_url,
    mac_name: s.mac_name,
    last_error: s.last_error,
    mode: s.mode,
    advertise_host: s.advertise_host,
    mobile_pairing_enabled: s.mobile_pairing_enabled,
}
```

(There are 4 places: `companion_get_status`, `companion_start_server`, `companion_stop_server`, and the new command below. Update all 4.)

Add the new command at the bottom of `commands/companion.rs`:

```rust
/// Toggle LAN mobile-pairing mode. Stops the running server (if any),
/// flips the preference, and restarts with the new bind configuration.
#[tauri::command]
pub async fn companion_set_mobile_pairing(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
) -> Result<CompanionStatusPayload, String> {
    // Persist the preference into the runtime before restart.
    {
        let mut rt = crate::companion::runtime().lock();
        rt.mobile_pairing_enabled = enabled;
    }
    // Stop, then restart with updated preference.
    crate::companion::stop().await;
    let app_state = state.inner().clone();
    let s = crate::companion::start(app_state, None, None).await?;
    Ok(CompanionStatusPayload {
        running: s.running,
        host: s.host,
        port: s.port,
        tls: s.tls,
        base_url: s.base_url,
        mac_name: s.mac_name,
        last_error: s.last_error,
        mode: s.mode,
        advertise_host: s.advertise_host,
        mobile_pairing_enabled: s.mobile_pairing_enabled,
    })
}
```

Note: `runtime()` is private in `mod.rs`. Add a small pub accessor:

```rust
// In src-tauri/src/companion/mod.rs
pub fn set_mobile_pairing_enabled(enabled: bool) {
    runtime().lock().mobile_pairing_enabled = enabled;
}
```

Then in the command use:

```rust
crate::companion::set_mobile_pairing_enabled(enabled);
```

- [ ] **Step 4: Register the command in `lib.rs`**

Find the `tauri::Builder` `.invoke_handler(tauri::generate_handler![...])` call in `src-tauri/src/lib.rs`. Add `companion_set_mobile_pairing` to the list (look for `companion_revoke_device` and add after it):

```rust
crate::ipc::commands::companion::companion_set_mobile_pairing,
```

- [ ] **Step 5: Run tests — expect PASS**

```bash
cd src-tauri && cargo test companion 2>&1 | tail -20
```

Expected: all companion tests pass.

- [ ] **Step 6: Add TypeScript IPC binding**

In `src/shared/ipc/tauri.ts`, update `CompanionStatusPayload`:

```typescript
export interface CompanionStatusPayload {
    running: boolean;
    host: string;
    port: number;
    tls: boolean;
    base_url: string;
    mac_name: string;
    last_error: string | null;
    mode: string;                   // "lan" | "loopback_only" | "lan_unavailable"
    advertise_host: string;
    mobile_pairing_enabled: boolean;
}
```

Add the new function after `companionRevokeDevice`:

```typescript
export async function companionSetMobilePairing(enabled: boolean): Promise<CompanionStatusPayload> {
    return invoke<CompanionStatusPayload>("companion_set_mobile_pairing", { enabled });
}
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/companion/mod.rs \
        src-tauri/src/ipc/commands/companion.rs \
        src-tauri/src/lib.rs \
        src/shared/ipc/tauri.ts
git commit -m "feat(companion): add mobile_pairing_enabled toggle command and mode field"
```

---

## Task 4: Mac TypeScript — mount `CompanionDevicesPanel` in ControlPanel

**Files:**
- Modify: `src/domains/workspace/ControlPanel.tsx`
- Modify: `src/domains/companion/__tests__/CompanionDevicesPanel.test.tsx`

- [ ] **Step 1: Write a failing Vitest test**

In `src/domains/companion/__tests__/CompanionDevicesPanel.test.tsx`, add a test that ControlPanel renders the panel when the companion tab is active. Because `ControlPanel` is large and Tauri-coupled, test the simpler assertion that `CompanionDevicesPanel` itself renders a status section when given mock data:

```typescript
it("renders mode diagnostic strip when status has mode field", async () => {
    vi.mocked(companionGetStatus).mockResolvedValue({
        running: true,
        host: "192.168.1.42",
        port: 47812,
        tls: true,
        base_url: "https://192.168.1.42:47812",
        mac_name: "Test Mac",
        last_error: null,
        mode: "lan",
        advertise_host: "192.168.1.42",
        mobile_pairing_enabled: true,
    });
    vi.mocked(companionListDevices).mockResolvedValue([]);

    render(<CompanionDevicesPanel pollIntervalMs={0} />);
    await waitFor(() => {
        expect(screen.getByText(/LAN/i)).toBeInTheDocument();
        expect(screen.getByText(/192\.168\.1\.42/)).toBeInTheDocument();
    });
});
```

- [ ] **Step 2: Run — expect FAIL**

```bash
pnpm test -- --reporter=verbose 2>&1 | grep -A 5 "mode diagnostic"
```

Expected: test fails because the panel doesn't render the mode strip yet.

- [ ] **Step 3: Add `CompanionDevicesPanel` as a "Paired Devices" tab in `ControlPanel.tsx`**

At the top of `src/domains/workspace/ControlPanel.tsx`, add the import:

```typescript
import { CompanionDevicesPanel } from "@/domains/companion/CompanionDevicesPanel";
```

Find the `type Tab = "settings" | "model" | "privacy";` line and extend it:

```typescript
type Tab = "settings" | "model" | "privacy" | "companion";
```

Find the three tab buttons (around line 693–709) and add a fourth:

```tsx
<button
    className={`ui-action-btn tab ${activeTab === "companion" ? "active" : ""}`}
    onClick={() => setActiveTab("companion")}
>
    Paired Devices
</button>
```

Find the tab content conditionals (after `{activeTab === "privacy" && ...}`) and add:

```tsx
{activeTab === "companion" && (
    <CompanionDevicesPanel />
)}
```

- [ ] **Step 4: Run tests — expect PASS**

```bash
pnpm test -- --reporter=verbose 2>&1 | grep -E "PASS|FAIL|mode diagnostic"
```

Expected: the mode diagnostic test passes (after implementing the strip in Task 5).

Note: the test above depends on the strip being rendered, so complete Task 5 before considering this test green.

- [ ] **Step 5: Commit (after Task 5 tests pass)**

Hold this commit until Task 5 is done — see Task 5 Step 6.

---

## Task 5: Mac TypeScript — diagnostic strip in `CompanionDevicesPanel`

**Files:**
- Modify: `src/domains/companion/CompanionDevicesPanel.tsx`

- [ ] **Step 1: Add the diagnostic strip to the panel render**

In `src/domains/companion/CompanionDevicesPanel.tsx`, find where the component renders its return JSX (after the `handleRevoke` and `handleStartPairing` callbacks). Add a `DiagnosticStrip` section above the pairing-code section:

```tsx
// Helper rendered at the top of the panel
function DiagnosticStrip({
    status,
    onToggle,
    toggling,
}: {
    status: CompanionStatusPayload;
    onToggle: () => void;
    toggling: boolean;
}) {
    const modeLabel =
        status.mode === "lan"
            ? "LAN"
            : status.mode === "lan_unavailable"
              ? "LAN (no Wi-Fi found)"
              : "Loopback only";

    const modeColor =
        status.mode === "lan" ? "#f59e0b" : status.mode === "lan_unavailable" ? "#ef4444" : "#6b7280";

    return (
        <div style={{ fontSize: 12, marginBottom: 12, padding: "8px 10px", background: "#1a1a1a", borderRadius: 6 }}>
            <div style={{ display: "flex", gap: 16, flexWrap: "wrap", marginBottom: 6 }}>
                <span>Bind: <code>{status.mobile_pairing_enabled ? "0.0.0.0" : "127.0.0.1"}</code></span>
                <span>Advertise: <code>{status.advertise_host || "—"}</code></span>
                <span>Port: <code>{status.port}</code></span>
                <span style={{ color: modeColor }}>Mode: {modeLabel}</span>
            </div>
            {status.mode === "lan" && (
                <div style={{ color: "#f59e0b", marginBottom: 6, fontSize: 11 }}>
                    ⚠ FNDR is reachable on your Wi-Fi. TLS + token-protected.
                </div>
            )}
            {status.mode === "lan_unavailable" && (
                <div style={{ color: "#ef4444", marginBottom: 6, fontSize: 11 }}>
                    Connect to Wi-Fi, then toggle off and on to retry.
                </div>
            )}
            <button onClick={onToggle} disabled={toggling} style={{ fontSize: 11 }}>
                {toggling
                    ? "Restarting…"
                    : status.mobile_pairing_enabled
                      ? "Disable mobile pairing"
                      : "Enable mobile pairing"}
            </button>
        </div>
    );
}
```

In the `CompanionDevicesPanel` function body, add state for the toggle and the handler:

```tsx
const [toggling, setToggling] = useState(false);

const handleToggleMobilePairing = useCallback(async () => {
    if (!status) return;
    setToggling(true);
    try {
        const next = !status.mobile_pairing_enabled;
        const updated = await companionSetMobilePairing(next);
        setStatus(updated);
    } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
    } finally {
        setToggling(false);
    }
}, [status]);
```

Add the import for `companionSetMobilePairing` at the top:

```typescript
import {
    // ... existing imports ...
    companionSetMobilePairing,
} from "@/shared/ipc/tauri";
```

In the JSX return, render `<DiagnosticStrip>` as the first element inside the panel (before the pairing section):

```tsx
{status && (
    <DiagnosticStrip
        status={status}
        onToggle={handleToggleMobilePairing}
        toggling={toggling}
    />
)}
```

Also disable the "Generate pairing code" button when mode is `lan_unavailable`:

```tsx
<button
    onClick={handleStartPairing}
    disabled={pairingInFlight || status?.mode === "lan_unavailable"}
    title={status?.mode === "lan_unavailable" ? "Connect to Wi-Fi to enable pairing" : undefined}
>
    {pairingInFlight ? "Generating…" : "Generate pairing code"}
</button>
```

- [ ] **Step 2: Run tests — expect PASS**

```bash
pnpm test 2>&1 | tail -20
```

Expected: all existing companion tests + the new mode-strip test pass.

- [ ] **Step 3: Commit Tasks 4 + 5 together**

```bash
git add src/domains/workspace/ControlPanel.tsx \
        src/domains/companion/CompanionDevicesPanel.tsx \
        src/domains/companion/__tests__/CompanionDevicesPanel.test.tsx \
        src/shared/ipc/tauri.ts
git commit -m "feat(companion): mount CompanionDevicesPanel in ControlPanel + diagnostic strip"
```

---

## Task 6: iOS Swift — add `mode` field to `QRPayload`

**Files:**
- Modify: `apps/ios/FNDRKit/Sources/FNDRKit/DTOs.swift`
- Modify: `apps/ios/FNDRKit/Sources/FNDRKitCheck/PairingFlowSuite.swift`

- [ ] **Step 1: Write a failing test**

In `apps/ios/FNDRKit/Sources/FNDRKitCheck/PairingFlowSuite.swift`, add at the top of the `pairingFlowSuite` array:

```swift
TestCase("QRPayload decodes mode field when present") {
    let json = """
    {
      "version": 1,
      "mac_name": "Test Mac",
      "host": "192.168.1.42",
      "port": 47812,
      "tls": true,
      "cert_fingerprint_sha256": "abcdef",
      "pairing_code": "381729",
      "expires_at_ms": 9999999999999,
      "mode": "lan"
    }
    """
    let payload = try PairingFlow.parseQRPayload(json)
    try expectEqual(payload.mode, "lan")
},

TestCase("QRPayload mode defaults to nil when absent") {
    // Older Mac versions won't send the mode field.
    let json = """
    {
      "version": 1,
      "mac_name": "Test Mac",
      "host": "192.168.1.42",
      "port": 47812,
      "tls": true,
      "pairing_code": "381729",
      "expires_at_ms": 9999999999999
    }
    """
    let payload = try PairingFlow.parseQRPayload(json)
    try expect(payload.mode == nil, "expected nil mode, got \(String(describing: payload.mode))")
},
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd apps/ios/FNDRKit && swift run FNDRKitCheck 2>&1 | tail -20
```

Expected: compile error — `QRPayload` has no `mode` field.

- [ ] **Step 3: Add `mode` to `QRPayload` in `DTOs.swift`**

In `apps/ios/FNDRKit/Sources/FNDRKit/DTOs.swift`, update the `QRPayload` struct:

```swift
public struct QRPayload: Codable, Equatable, Sendable {
    public let version: Int
    public let macName: String
    public let host: String
    public let port: Int
    public let tls: Bool
    public let certFingerprintSha256: String?
    public let pairingCode: String
    public let expiresAtMs: Int64
    public let mode: String?       // ← new: "lan" | "loopback_only" | "lan_unavailable" | nil (old servers)

    enum CodingKeys: String, CodingKey {
        case version
        case macName = "mac_name"
        case host
        case port
        case tls
        case certFingerprintSha256 = "cert_fingerprint_sha256"
        case pairingCode = "pairing_code"
        case expiresAtMs = "expires_at_ms"
        case mode
    }
}
```

- [ ] **Step 4: Run — expect PASS**

```bash
cd apps/ios/FNDRKit && swift run FNDRKitCheck 2>&1 | tail -20
```

Expected: all tests pass including the two new `QRPayload` mode tests.

- [ ] **Step 5: Commit**

```bash
git add apps/ios/FNDRKit/Sources/FNDRKit/DTOs.swift \
        apps/ios/FNDRKit/Sources/FNDRKitCheck/PairingFlowSuite.swift
git commit -m "feat(ios): add mode field to QRPayload DTO"
```

---

## Task 7: iOS Swift — `PairingFlow` loopback rejection + mode validation

**Files:**
- Modify: `apps/ios/FNDRKit/Sources/FNDRKit/PairingFlow.swift`
- Modify: `apps/ios/FNDRKit/Sources/FNDRKitCheck/PairingFlowSuite.swift`

- [ ] **Step 1: Write the failing tests**

Add these three cases to `pairingFlowSuite` in `PairingFlowSuite.swift`:

```swift
TestCase("accept rejects loopback host when not simulator") {
    // Simulate physical-device environment by passing isSimulator: false.
    let json = """
    {
      "version": 1, "mac_name": "Mac", "host": "127.0.0.1", "port": 47812,
      "tls": true, "pairing_code": "123456", "expires_at_ms": 9999999999999
    }
    """
    let payload = try PairingFlow.parseQRPayload(json)
    let flow = PairingFlow(keychain: InMemoryKeychainStore(), now: { 1 }, isSimulator: false)
    let state = await flow.accept(payload: payload)
    guard case .failed(let msg) = state else {
        try expect(false, "expected .failed, got \(state)")
        return
    }
    try expect(msg.contains("simulator only"), "wrong error: \(msg)")
},

TestCase("accept rejects loopback_only mode regardless of host") {
    let json = """
    {
      "version": 1, "mac_name": "Mac", "host": "192.168.1.42", "port": 47812,
      "tls": true, "pairing_code": "123456", "expires_at_ms": 9999999999999,
      "mode": "loopback_only"
    }
    """
    let payload = try PairingFlow.parseQRPayload(json)
    // mode check applies on any environment
    let flow = PairingFlow(keychain: InMemoryKeychainStore(), now: { 1 }, isSimulator: true)
    let state = await flow.accept(payload: payload)
    guard case .failed(let msg) = state else {
        try expect(false, "expected .failed, got \(state)")
        return
    }
    try expect(msg.contains("simulator only"), "wrong error: \(msg)")
},

TestCase("accept allows loopback host in simulator environment") {
    let json = """
    {
      "version": 1, "mac_name": "Mac", "host": "127.0.0.1", "port": 47812,
      "tls": true, "pairing_code": "123456", "expires_at_ms": 9999999999999
    }
    """
    let payload = try PairingFlow.parseQRPayload(json)
    let flow = PairingFlow(keychain: InMemoryKeychainStore(), now: { 1 }, isSimulator: true)
    let state = await flow.accept(payload: payload)
    try expectEqual(state, .ready(payload))
},
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd apps/ios/FNDRKit && swift run FNDRKitCheck 2>&1 | tail -20
```

Expected: compile error — `PairingFlow.init` has no `isSimulator` parameter.

- [ ] **Step 3: Add `isSimulator` to `PairingFlow` and implement the new validation rules**

In `apps/ios/FNDRKit/Sources/FNDRKit/PairingFlow.swift`, update the actor:

```swift
public actor PairingFlow {
    private let keychain: KeychainStorage
    private let now: @Sendable () -> Int64
    private let isSimulator: Bool      // ← new
    private let transportFactory: @Sendable (QRPayload) -> CompanionTransport
    private let clientFactory: @Sendable (CompanionClient.Configuration, CompanionTransport) -> CompanionClient

    public private(set) var state: PairingState = .idle

    public init(
        keychain: KeychainStorage,
        now: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) },
        isSimulator: Bool = {               // ← new parameter with default from compile-time flag
            #if targetEnvironment(simulator)
            return true
            #else
            return false
            #endif
        }(),
        transportFactory: @escaping @Sendable (QRPayload) -> CompanionTransport = { payload in
            URLSessionTransport(pinnedFingerprint: payload.certFingerprintSha256)
        },
        clientFactory: @escaping @Sendable (CompanionClient.Configuration, CompanionTransport) -> CompanionClient = {
            CompanionClient(config: $0, transport: $1)
        }
    ) {
        self.keychain = keychain
        self.now = now
        self.isSimulator = isSimulator
        self.transportFactory = transportFactory
        self.clientFactory = clientFactory
    }
```

Update `accept(payload:)` — add new validation rules after the existing pairing-code check:

```swift
public func accept(payload: QRPayload) -> PairingState {
    let expired = payload.expiresAtMs <= now()
    if expired {
        state = .failed(message: "Pairing code expired — generate a new one on the Mac.")
    } else if payload.version != 1 {
        state = .failed(message: "Unsupported QR payload version (\(payload.version)).")
    } else if payload.pairingCode.count != 6 || !payload.pairingCode.allSatisfy({ $0.isNumber }) {
        state = .failed(message: "Pairing code must be six digits.")
    } else if payload.host.isEmpty {
        state = .failed(message: "Pairing payload has no host.")
    } else if payload.mode == "loopback_only" {
        // Mode-field check: applies on any environment.
        state = .failed(message: "This pairing code is for simulator only — enable mobile pairing in FNDR Mac Settings, then paste a new code.")
    } else if !isSimulator && ["127.0.0.1", "::1", "localhost"].contains(payload.host) {
        // Loopback host on a physical device.
        state = .failed(message: "This pairing code is for simulator only — enable mobile pairing in FNDR Mac Settings, then paste a new code.")
    } else {
        state = .ready(payload)
    }
    return state
}
```

- [ ] **Step 4: Run — expect PASS**

```bash
cd apps/ios/FNDRKit && swift run FNDRKitCheck 2>&1 | tail -20
```

Expected: all tests pass including the 3 new pairing validation cases.

- [ ] **Step 5: Commit**

```bash
git add apps/ios/FNDRKit/Sources/FNDRKit/PairingFlow.swift \
        apps/ios/FNDRKit/Sources/FNDRKitCheck/PairingFlowSuite.swift
git commit -m "feat(ios): PairingFlow rejects loopback host and loopback_only mode on physical device"
```

---

## Task 8: iOS Swift — connection probe before Keychain persist

**Files:**
- Modify: `apps/ios/FNDRKit/Sources/FNDRKit/PairingFlow.swift`
- Modify: `apps/ios/FNDRKit/Sources/FNDRKitCheck/PairingFlowSuite.swift`

- [ ] **Step 1: Write the failing test**

The existing `StubTransport` only handles one response. Create a `SequenceTransport` for multi-step mocking. Add at the top of `PairingFlowSuite.swift`:

```swift
private actor SequenceTransport: CompanionTransport {
    private var responses: [(status: Int, body: Data)]
    init(_ responses: [(Int, Data)]) { self.responses = responses }

    func send(request: URLRequest) async throws -> (Data, URLResponse) {
        guard !responses.isEmpty else {
            throw URLError(.badServerResponse)
        }
        let (status, body) = responses.removeFirst()
        let resp = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!
        return (body, resp)
    }
}
```

Add the test case to `pairingFlowSuite`:

```swift
TestCase("complete does not persist token when status probe fails") {
    let pairResponseData = try JSONEncoder().encode(PairCompleteResponse(
        deviceId: "dev_1",
        accessToken: "tok_abc",
        macName: "Test Mac",
        permissions: ["ask"]
    ))
    // Step 1 (pair/complete) → 200; Step 2 (status probe) → 503
    let transport = SequenceTransport([(200, pairResponseData), (503, Data())])
    let keychain = InMemoryKeychainStore()
    let flow = PairingFlow(
        keychain: keychain,
        now: { 1 },
        isSimulator: true,
        transportFactory: { _ in transport },
        clientFactory: { CompanionClient(config: $0, transport: $1) }
    )
    let json = """
    {
      "version": 1, "mac_name": "Mac", "host": "127.0.0.1", "port": 47812,
      "tls": false, "pairing_code": "123456", "expires_at_ms": 9999999999999
    }
    """
    let payload = try PairingFlow.parseQRPayload(json)
    _ = await flow.accept(payload: payload)
    _ = await flow.complete(deviceName: "iPhone", deviceType: .iphone, appVersion: nil)

    let finalState = await flow.state
    guard case .failed(let msg) = finalState else {
        try expect(false, "expected .failed, got \(finalState)")
        return
    }
    try expect(msg.contains("didn't respond"), "wrong error message: \(msg)")
    // Keychain must be empty — token must NOT have been written.
    let token = try? keychain.stringForKey(KeychainKeys.accessToken)
    try expect(token == nil, "token was written despite probe failure")
},
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd apps/ios/FNDRKit && swift run FNDRKitCheck 2>&1 | tail -20
```

Expected: test fails — current code writes the keychain before probing.

- [ ] **Step 3: Implement the connection probe in `PairingFlow.complete()`**

In `PairingFlow.swift`, update `complete(deviceName:deviceType:appVersion:)`. The probe goes between receiving the `PairCompleteResponse` and writing to the keychain:

```swift
@discardableResult
public func complete(deviceName: String, deviceType: DeviceType, appVersion: String?) async -> PairingState {
    guard case .ready(let payload) = state else {
        state = .failed(message: "No pairing payload to complete.")
        return state
    }
    state = .pairing(payload)

    let baseURL = baseURL(for: payload)
    let transport = transportFactory(payload)
    let client = clientFactory(.init(baseURL: baseURL, accessToken: nil), transport)
    let request = PairCompleteRequest(
        pairingCode: payload.pairingCode,
        deviceName: deviceName,
        deviceType: deviceType,
        appVersion: appVersion
    )

    do {
        let response = try await client.completePairing(request: request)

        // ── connection probe ──────────────────────────────────────────────
        // Build a temporary client with the new token and verify the Mac
        // answers before committing anything to the Keychain.
        let probedPairedURL = baseURL  // same URL, already resolved
        let probeTransport = transportFactory(payload)
        let probeClient = clientFactory(
            .init(baseURL: probedPairedURL, accessToken: response.accessToken),
            probeTransport
        )
        do {
            _ = try await probeClient.status()
        } catch {
            state = .failed(message: "Pairing token issued but the Mac didn't respond at \(payload.host):\(payload.port). Are both devices on the same Wi-Fi?")
            return state
        }
        // ── end probe ─────────────────────────────────────────────────────

        let paired = PairedMac(
            deviceId: response.deviceId,
            macName: response.macName,
            host: payload.host,
            port: payload.port,
            tls: payload.tls,
            certFingerprintSha256: payload.certFingerprintSha256,
            permissions: response.permissions,
            pairedAtMs: now()
        )

        do {
            try keychain.setString(response.accessToken, forKey: KeychainKeys.accessToken)
            try keychain.setCodable(paired, forKey: KeychainKeys.pairedMac)
        } catch {
            state = .failed(message: "Pairing succeeded but token storage failed: \(error.localizedDescription)")
            return state
        }

        state = .paired(paired)
        return state

    } catch CompanionError.pairingCodeInvalid {
        state = .failed(message: "Pairing code is invalid or expired.")
        return state
    } catch CompanionError.pairingCodeUsed {
        state = .failed(message: "Pairing code was already used — generate a new one on the Mac.")
        return state
    } catch let CompanionError.tlsFingerprintMismatch(expected, _) {
        state = .failed(message: "TLS fingerprint mismatch against the paired Mac (\(expected.prefix(12))…). Re-pair.")
        return state
    } catch {
        state = .failed(message: error.localizedDescription)
        return state
    }
}
```

Note: `probeClient.status()` calls `GET /v1/status`. Verify that `CompanionClient` has a `status()` method — it should in `CompanionClient.swift`. If not, use `probeClient.health()` if that exists, or add a minimal status call:

```swift
// In CompanionClient.swift, if status() doesn't exist, add:
public func status() async throws -> StatusResponse {
    return try await get(path: "/v1/status")
}
```

- [ ] **Step 4: Run — expect PASS**

```bash
cd apps/ios/FNDRKit && swift run FNDRKitCheck 2>&1 | tail -20
```

Expected: all tests pass including the new connection-probe test.

- [ ] **Step 5: Commit**

```bash
git add apps/ios/FNDRKit/Sources/FNDRKit/PairingFlow.swift \
        apps/ios/FNDRKit/Sources/FNDRKitCheck/PairingFlowSuite.swift
git commit -m "feat(ios): probe /v1/status before persisting pairing token to Keychain"
```

---

## Task 9: iOS — `project.yml` Watch fix, `Local.xcconfig`, and `make ios-bootstrap`

**Files:**
- Modify: `apps/ios/project.yml`
- Create: `apps/ios/Local.xcconfig.example`
- Modify: `Makefile`
- Modify: `.gitignore` (add `apps/ios/Local.xcconfig` if not already there)

This task has no TDD cycle (XcodeGen is not unit-testable). Verification is by running `xcodegen generate` and confirming no duplicate-target warnings.

- [ ] **Step 1: Fix the Watch target type in `project.yml`**

Open `apps/ios/project.yml`. Find the `FNDR Watch:` target. Change:

```yaml
  FNDR Watch:
    type: application.watchapp2    # ← old
```

to:

```yaml
  FNDR Watch:
    type: application              # ← modern single-target watchOS app
    platform: watchOS
```

Confirm these keys remain in the Watch target's `settings.base` block:

```yaml
        INFOPLIST_KEY_WKApplication: YES
        INFOPLIST_KEY_WKCompanionAppBundleIdentifier: $(BUNDLE_ID_PREFIX).ios
```

- [ ] **Step 2: Add `configFiles` and bundle ID variables to both targets**

Replace the hardcoded `com.fndr.ios` values and add `configFiles`. The full updated `project.yml` targets section:

```yaml
targets:
  FNDR:
    type: application
    platform: iOS
    deploymentTarget: '17.0'
    sources:
      - path: FNDR
    dependencies:
      - package: FNDRKit
        product: FNDRKit
      - target: FNDR Watch
        embed: true
    configFiles:
      Debug: Local.xcconfig
      Release: Local.xcconfig
    settings:
      base:
        INFOPLIST_KEY_CFBundleDisplayName: FNDR
        INFOPLIST_KEY_UILaunchScreen_Generation: YES
        INFOPLIST_KEY_UIApplicationSceneManifest_Generation: YES
        INFOPLIST_KEY_UIStatusBarStyle: UIStatusBarStyleDefault
        INFOPLIST_KEY_NSLocalNetworkUsageDescription: FNDR needs local network access to connect to your Mac companion runtime.
        CODE_SIGN_STYLE: Automatic
        TARGETED_DEVICE_FAMILY: "1,2"
        GENERATE_INFOPLIST_FILE: YES
        PRODUCT_BUNDLE_IDENTIFIER: $(BUNDLE_ID_PREFIX).ios
        # DEVELOPMENT_TEAM comes from Local.xcconfig

  FNDR Watch:
    type: application
    platform: watchOS
    deploymentTarget: '10.0'
    sources:
      - path: "FNDR Watch"
    dependencies:
      - package: FNDRKit
        product: FNDRKit
    configFiles:
      Debug: Local.xcconfig
      Release: Local.xcconfig
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: $(BUNDLE_ID_PREFIX).ios.watchkitapp
        INFOPLIST_KEY_CFBundleDisplayName: FNDR Watch
        INFOPLIST_KEY_WKCompanionAppBundleIdentifier: $(BUNDLE_ID_PREFIX).ios
        INFOPLIST_KEY_WKApplication: YES
        CODE_SIGN_STYLE: Automatic
        GENERATE_INFOPLIST_FILE: YES
        # DEVELOPMENT_TEAM comes from Local.xcconfig
```

Remove the `DEVELOPMENT_TEAM: ""` lines from both targets (it comes from xcconfig now).

- [ ] **Step 3: Create `apps/ios/Local.xcconfig.example`**

```bash
cat > apps/ios/Local.xcconfig.example << 'EOF'
// Copy this file to Local.xcconfig and fill in your values.
// Local.xcconfig is gitignored — never commit the real file.
// Find your Team ID at: https://developer.apple.com/account → Membership
DEVELOPMENT_TEAM =
BUNDLE_ID_PREFIX =
EOF
```

- [ ] **Step 4: Add `apps/ios/Local.xcconfig` to `.gitignore`**

Verify it's there (Task 1 should have added it). If not:

```bash
echo "apps/ios/Local.xcconfig" >> .gitignore
```

- [ ] **Step 5: Add `ios-bootstrap` to `Makefile`**

Add after the existing `install:` target:

```makefile
ios-bootstrap:
	@echo "=== FNDR iOS local signing setup ==="
	@read -p "Apple Developer Team ID (10 chars from developer.apple.com/account): " team; \
	 read -p "Bundle ID prefix (e.g. com.yourname): " prefix; \
	 printf "DEVELOPMENT_TEAM = %s\nBUNDLE_ID_PREFIX = %s\n" "$$team" "$$prefix" \
	   > apps/ios/Local.xcconfig; \
	 echo "Written apps/ios/Local.xcconfig (gitignored)."
	xcodegen generate --spec apps/ios/project.yml --project apps/ios
	@echo "Done. Open apps/ios/FNDR.xcodeproj in Xcode."
```

Ensure this is under `.PHONY`:

```makefile
.PHONY: ... ios-bootstrap
```

- [ ] **Step 6: Verify with `xcodegen generate`**

First create a local `Local.xcconfig` for yourself (this file is gitignored):

```bash
printf "DEVELOPMENT_TEAM = XXXXXXXXXX\nBUNDLE_ID_PREFIX = com.test\n" > apps/ios/Local.xcconfig
```

Then regenerate:

```bash
xcodegen generate --spec apps/ios/project.yml --project apps/ios 2>&1
```

Expected: output ends with `⚙️  Generating project...` and no errors about duplicate targets or missing keys.

Delete the test xcconfig after verification:

```bash
rm apps/ios/Local.xcconfig
```

- [ ] **Step 7: Verify `Local.xcconfig` is gitignored**

```bash
git ls-files apps/ios/Local.xcconfig
```

Expected: no output (file is not tracked).

- [ ] **Step 8: Commit**

```bash
git add apps/ios/project.yml apps/ios/Local.xcconfig.example Makefile .gitignore
git commit -m "feat(ios): fix Watch target type, add Local.xcconfig signing override, ios-bootstrap make target"
```

---

## Task 10: Runbook

**Files:**
- Create: `docs/companion/real-device-runbook.md`

- [ ] **Step 1: Create the runbook**

```bash
cat > docs/companion/real-device-runbook.md << 'RUNBOOK'
# FNDR — Real-Device iPhone + Apple Watch Install Runbook

Personal-device install using free Apple ID signing. Certificate lifetime: 7 days.

---

## Prerequisites

- **Xcode** full app (not Command Line Tools only). Check: `xcodebuild -version`
- **xcodegen**: `brew install xcodegen`
- **Apple ID** (free personal team is sufficient). Find your Team ID at developer.apple.com → Account → Membership (10-char alphanumeric string).
- **Apple Watch** paired to the iPhone you're targeting, running watchOS 10+.
- **Mac and iPhone** on the same home Wi-Fi network. Router must not have client isolation (devices can reach each other).
- **FNDR repo** cloned and on `companion/slice-2-ios-shell` or a branch that contains these changes.

---

## One-Time Setup

```bash
make ios-bootstrap
```

This prompts for your Team ID and bundle prefix, writes `apps/ios/Local.xcconfig` (gitignored), and runs `xcodegen generate`. You only need to run this once per machine; re-run it if you change your signing identity.

---

## Start the Mac Runtime

```bash
npm run tauri dev
```

1. Open FNDR → click the ⚙️ Settings icon → **Paired Devices** tab.
2. Look at the diagnostic strip:
   - **Mode: Loopback only** → click **Enable mobile pairing**. Wait ~2 s for the server to restart.
   - **Mode: LAN** → you're ready. The advertised host should be a `192.168.x.y` address, not `127.0.0.1`.
   - **Mode: LAN (no Wi-Fi found)** → connect the Mac to Wi-Fi, then toggle mobile pairing off and on.
3. Click **Generate pairing code**. Copy the full QR JSON payload shown on screen.

---

## iPhone First Install

1. Plug the iPhone into the Mac with a USB cable.
2. Open `apps/ios/FNDR.xcodeproj` in Xcode.
3. Select the **FNDR** scheme and your physical iPhone as the destination.
4. Press **Run** (▶). Xcode allocates a provisioning profile automatically using your Team ID.
5. First launch: iOS shows **"Untrusted Developer."** Go to:
   **Settings → VPN & Device Management → Developer App → [your Apple ID] → Trust**
6. Press **Run** again from Xcode. The app opens on the device.

---

## Pair the iPhone

1. In the FNDR app tap the **Pair** option (on the pairing screen or Settings tab).
2. Paste the QR JSON you copied from the Mac.
3. Tap **Validate payload**. You should see no error. If you see "simulator only" → go back to the Mac, verify mobile pairing is ON and the advertised host is a LAN IP, regenerate the code.
4. Tap **Complete pairing**. The app calls the Mac, probes `/v1/status`, and saves the token.
5. The Status tab should show live data (capture status, runtime status, storage) within ~3 s.

---

## Watch First Install

The iPhone app must be installed first.

1. In Xcode, switch the scheme to **FNDR Watch**.
2. Set the destination to your paired Apple Watch.
3. Press **Run**. Xcode installs the Watch app via the paired iPhone.
4. If the Watch app does not appear: open the **Watch app on iPhone** → **My Watch** tab → scroll to FNDR → tap **Install**.

---

## Smoke Checklist

Run these and record pass/fail in the PR description.

**iPhone:**
- [ ] Status tab shows capture status, runtime status, storage status (all populated).
- [ ] Capture tab: submit a manual note → it appears in the Mac's vault.
- [ ] Disable Wi-Fi on iPhone → Status tab shows "unreachable."
- [ ] Re-enable Wi-Fi → next Status refresh succeeds.
- [ ] Settings → "Clear pairing" → app returns to pairing screen.

**Watch:**
- [ ] FNDR Watch → Status screen shows data (via WCSession → iPhone → Mac).
- [ ] FNDR Watch → Remember screen: enter a note → it appears in the Mac's vault.

**Mac diagnostics:**
- [ ] Mobile pairing OFF → strip shows "Loopback only," generate button active (code works in simulator only).
- [ ] Mobile pairing ON → strip shows "LAN" with a non-loopback IP.
- [ ] Mobile pairing ON + Mac disconnected from Wi-Fi → strip shows "LAN (no Wi-Fi found)," generate button disabled.

**Negative (physical device):**
- [ ] Paste a loopback QR (host: 127.0.0.1) into the physical iPhone → rejected with "simulator only" message.

---

## Failure-Mode Index

| Symptom | Cause | Fix |
|---------|-------|-----|
| "Untrusted Developer" on iPhone | Free-signing cert not trusted | Settings → VPN & Device Management → Developer App → Trust |
| "Unable to Verify App" on launch | 7-day free-signing cert expired | Re-deploy from Xcode (build & run to the device). No re-pairing needed unless you also revoked the device. |
| Pairing rejected: "simulator only" | QR has loopback host or `mode: loopback_only` | Enable mobile pairing on Mac → verify diagnostic strip shows LAN IP → regenerate code |
| "Mac didn't respond" after pairing | Status probe failed — different Wi-Fi or wrong IP | Ensure Mac and iPhone are on the same network; check the Mac's diagnostic strip for the current IP |
| Watch app missing from Watch | iPhone target not installed first | Install FNDR on iPhone first; then install Watch scheme |
| Status tab stuck "unreachable" after Wi-Fi change | Mac's DHCP IP changed | Tap "Clear pairing" on iPhone → re-pair with a new code (Mac's current IP is in the diagnostic strip) |
| TLS fingerprint mismatch | Mac cert regenerated since last pairing | Clear pairing → re-pair; the new QR carries the new fingerprint |
| Diagnostic strip shows wrong IP | Auto-resolver picked a VPN/Ethernet interface | Verify you're on Wi-Fi, not Ethernet; if needed, run `make ios-bootstrap` again (no effect) or restart FNDR on Mac |
| `xcodegen generate` warns: duplicate targets | Old `application.watchapp2` target type | Confirm `project.yml` FNDR Watch uses `type: application` (Task 9) |

---

## Weekly Re-Deploy (Free Signing Expiry)

Free-signing certificates expire after 7 days. When iOS shows "Unable to Verify App":

1. Plug iPhone into Mac.
2. Open Xcode → select `FNDR` scheme + iPhone destination → Run (▶).
3. Xcode renews the cert automatically. Trust in Settings if prompted.
4. Repeat for `FNDR Watch` scheme if the Watch app also expired.

No re-pairing required (Keychain token survives re-deploy) unless you also revoked the device from Mac Settings.
RUNBOOK
```

- [ ] **Step 2: Commit**

```bash
git add docs/companion/real-device-runbook.md
git commit -m "docs(companion): real-device iPhone + Watch install runbook"
```

---

## Task 11: Final verification

- [ ] **Step 1: Run all Rust tests**

```bash
cd src-tauri && cargo test 2>&1 | tail -30
```

Expected: all tests pass. Zero compile errors. The four out-of-scope files (`lance_store/arrow_and_filters.rs`, `lance_store/schemas.rs`, `storage/schema.rs`, `telemetry/system_metrics.rs`) are unchanged — verify with:

```bash
git diff --name-only HEAD~1 | grep -E "lance_store|schema\.rs|system_metrics"
```

Expected: no output.

- [ ] **Step 2: Run Swift tests**

```bash
cd apps/ios/FNDRKit && swift run FNDRKitCheck 2>&1 | tail -30
```

Expected: all suites pass, zero failures.

- [ ] **Step 3: Run TypeScript tests**

```bash
pnpm test 2>&1 | tail -20
```

Expected: all Vitest tests pass.

- [ ] **Step 4: Gitignore hygiene check**

```bash
git ls-files | grep -E 'xcuserdata|DerivedData|xcuserstate|xcresult|Local\.xcconfig|\.swiftpm'
```

Expected: no output.

```bash
git ls-files apps/ios/Local.xcconfig
```

Expected: no output (file is gitignored).

```bash
git ls-files apps/ios/Local.xcconfig.example
```

Expected: `apps/ios/Local.xcconfig.example` (file IS tracked).

- [ ] **Step 5: Verify acceptance criteria from spec**

Check each item in `docs/superpowers/specs/2026-05-25-real-device-companion-design.md` Section G against the implementation:

```
[ ] companion_set_mobile_pairing command exists → grep src-tauri/src/ipc/commands/companion.rs
[ ] mode field flows Rust → QrPayload JSON → Swift QRPayload → accept() validation
[ ] CompanionDevicesPanel mounted in ControlPanel under "Paired Devices" tab
[ ] PairingFlow rejects loopback on physical device (isSimulator: false path)
[ ] PairingFlow rejects mode == "loopback_only" on any device
[ ] PairingFlow probes /v1/status before keychain write
[ ] apps/ios/project.yml Watch target uses type: application
[ ] apps/ios/Local.xcconfig.example committed
[ ] real-device-runbook.md exists
```

- [ ] **Step 6: Real-device smoke (run manually, record in PR)**

Follow `docs/companion/real-device-runbook.md` from start to finish. Record each smoke checklist item as pass/fail in the PR description for `companion/slice-2-ios-shell`.

- [ ] **Step 7: Final commit and push**

```bash
git push origin companion/slice-2-ios-shell
```
