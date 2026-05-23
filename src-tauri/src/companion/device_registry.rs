//! Persistent registry of paired iPhone/Watch devices.
//!
//! Backed by `StateStore` under the key `companion_devices`. The whole list is
//! loaded into memory on demand; mutations rewrite the list. Mobile pairing is
//! a low-cardinality operation (a handful of devices per Mac), so a Vec is
//! plenty.

use crate::companion::dto::MobileDevice;
use crate::companion::errors::{CompanionError, CompanionResult};
use crate::storage::StateStore;
use parking_lot::RwLock;
use std::sync::Arc;

const STATE_KEY: &str = "companion_devices";

/// In-memory cache + StateStore-backed durability.
///
/// The cache is the source of truth between writes; we re-read from StateStore
/// only on construction. Writes flush the whole list to disk.
pub struct DeviceRegistry {
    state_store: Arc<StateStore>,
    devices: RwLock<Vec<MobileDevice>>,
}

impl DeviceRegistry {
    pub fn new(state_store: Arc<StateStore>) -> CompanionResult<Self> {
        let devices = state_store
            .load_json::<Vec<MobileDevice>>(STATE_KEY)
            .map_err(CompanionError::Internal)?
            .unwrap_or_default();

        Ok(Self {
            state_store,
            devices: RwLock::new(devices),
        })
    }

    /// Append a new device and persist. Tokens must be unique.
    pub fn insert(&self, device: MobileDevice) -> CompanionResult<()> {
        let mut devices = self.devices.write();
        if devices
            .iter()
            .any(|d| d.access_token == device.access_token)
        {
            return Err(CompanionError::Internal(
                "duplicate access token generated".to_string(),
            ));
        }
        devices.push(device);
        self.state_store
            .save_json(STATE_KEY, &*devices)
            .map_err(CompanionError::Internal)
    }

    /// Look up an active (non-revoked) device by access token.
    ///
    /// Also bumps `last_seen_at_ms`. The bump is best-effort — if persistence
    /// fails we still return the device because the request is authenticated.
    pub fn find_by_token(&self, token: &str, now_ms: i64) -> Option<MobileDevice> {
        let mut devices = self.devices.write();
        let idx = devices
            .iter()
            .position(|d| d.access_token == token && d.is_active())?;
        devices[idx].last_seen_at_ms = now_ms;
        let device = devices[idx].clone();
        // Best-effort flush. Don't fail the request on disk hiccups.
        if let Err(err) = self.state_store.save_json(STATE_KEY, &*devices) {
            tracing::warn!(error = %err, "failed to persist device last_seen_at_ms bump");
        }
        Some(device)
    }

    /// Mark a device revoked. Idempotent on devices that don't exist.
    pub fn revoke(&self, device_id: &str, now_ms: i64) -> CompanionResult<bool> {
        let mut devices = self.devices.write();
        let Some(idx) = devices.iter().position(|d| d.device_id == device_id) else {
            return Ok(false);
        };
        if devices[idx].revoked_at_ms.is_some() {
            return Ok(false);
        }
        devices[idx].revoked_at_ms = Some(now_ms);
        self.state_store
            .save_json(STATE_KEY, &*devices)
            .map_err(CompanionError::Internal)?;
        Ok(true)
    }

    pub fn list(&self) -> Vec<MobileDevice> {
        self.devices.read().clone()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.devices.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::dto::DeviceType;
    use tempfile::TempDir;

    fn fresh_registry() -> (TempDir, DeviceRegistry) {
        let tmp = TempDir::new().unwrap();
        let store = Arc::new(StateStore::new(tmp.path()).unwrap());
        let reg = DeviceRegistry::new(store).unwrap();
        (tmp, reg)
    }

    fn sample_device(id: &str, token: &str) -> MobileDevice {
        MobileDevice {
            device_id: id.to_string(),
            device_name: "iPhone of Test".to_string(),
            device_type: DeviceType::Iphone,
            access_token: token.to_string(),
            paired_at_ms: 1_700_000_000_000,
            last_seen_at_ms: 1_700_000_000_000,
            permissions: vec!["ask".to_string()],
            revoked_at_ms: None,
            public_key: None,
            app_version: None,
        }
    }

    #[test]
    fn insert_and_find_by_token_round_trip() {
        let (_tmp, reg) = fresh_registry();
        reg.insert(sample_device("dev_1", "tok_a")).unwrap();
        let found = reg.find_by_token("tok_a", 1_700_000_010_000).unwrap();
        assert_eq!(found.device_id, "dev_1");
        assert_eq!(found.last_seen_at_ms, 1_700_000_010_000);
    }

    #[test]
    fn find_by_token_misses_unknown_token() {
        let (_tmp, reg) = fresh_registry();
        reg.insert(sample_device("dev_1", "tok_a")).unwrap();
        assert!(reg.find_by_token("does_not_exist", 1).is_none());
    }

    #[test]
    fn revoked_device_is_not_findable() {
        let (_tmp, reg) = fresh_registry();
        reg.insert(sample_device("dev_1", "tok_a")).unwrap();
        let revoked = reg.revoke("dev_1", 1_700_000_999_000).unwrap();
        assert!(revoked);
        assert!(reg.find_by_token("tok_a", 1_700_000_999_001).is_none());
    }

    #[test]
    fn revoke_returns_false_for_missing_device() {
        let (_tmp, reg) = fresh_registry();
        let revoked = reg.revoke("does_not_exist", 1).unwrap();
        assert!(!revoked);
    }

    #[test]
    fn revoke_is_idempotent() {
        let (_tmp, reg) = fresh_registry();
        reg.insert(sample_device("dev_1", "tok_a")).unwrap();
        assert!(reg.revoke("dev_1", 1).unwrap());
        // Second call returns false (already revoked) but doesn't error.
        assert!(!reg.revoke("dev_1", 2).unwrap());
    }

    #[test]
    fn devices_persist_across_registry_reinit() {
        let tmp = TempDir::new().unwrap();
        let store = Arc::new(StateStore::new(tmp.path()).unwrap());
        let reg1 = DeviceRegistry::new(store.clone()).unwrap();
        reg1.insert(sample_device("dev_1", "tok_a")).unwrap();
        reg1.insert(sample_device("dev_2", "tok_b")).unwrap();
        drop(reg1);

        let reg2 = DeviceRegistry::new(store).unwrap();
        assert_eq!(reg2.len(), 2);
        assert!(reg2.find_by_token("tok_a", 0).is_some());
        assert!(reg2.find_by_token("tok_b", 0).is_some());
    }

    #[test]
    fn duplicate_token_insert_returns_internal() {
        let (_tmp, reg) = fresh_registry();
        reg.insert(sample_device("dev_1", "tok_a")).unwrap();
        let err = reg.insert(sample_device("dev_2", "tok_a")).unwrap_err();
        assert!(matches!(err, CompanionError::Internal(_)));
    }
}
