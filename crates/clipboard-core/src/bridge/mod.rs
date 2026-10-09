mod errors;
mod types;

pub use errors::ClipSyncError;
pub use types::{BridgeConfig, DeviceIdentity};

use std::sync::Mutex;

use crate::runtime::{start, AppHandle};

pub struct ClipSyncCore {
    handle: Mutex<Option<AppHandle>>,
}

impl ClipSyncCore {
    pub fn start(config: BridgeConfig) -> Result<ClipSyncCore, ClipSyncError> {
        let app_config = config.into_app_config()?;
        let handle = start(app_config).map_err(ClipSyncError::from)?;
        Ok(ClipSyncCore {
            handle: Mutex::new(Some(handle)),
        })
    }

    pub fn identity(&self) -> Result<DeviceIdentity, ClipSyncError> {
        let guard = self
            .handle
            .lock()
            .map_err(|_| ClipSyncError::InternalError)?;
        let handle = guard.as_ref().ok_or(ClipSyncError::CoreStopped)?;
        let (device_id, device_name) = handle.identity();
        Ok(DeviceIdentity {
            device_id: device_id.to_string(),
            device_name,
        })
    }

    pub fn stop(&self) {
        if let Ok(mut guard) = self.handle.lock() {
            if let Some(handle) = guard.take() {
                handle.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_port(port: u16) -> u16 {
        port + (std::process::id() % 1_000) as u16
    }

    fn temp_storage_path() -> String {
        std::env::temp_dir()
            .join(format!(
                "clipsync_bridge_test_{}_{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ))
            .display()
            .to_string()
    }

    fn valid_config() -> BridgeConfig {
        BridgeConfig {
            storage_path: temp_storage_path(),
            discovery_port: test_port(58271),
            listen_port: test_port(58272),
            discovery_targets: Vec::new(),
            sync_enabled: true,
        }
    }

    #[test]
    fn empty_storage_path_is_rejected() {
        let config = BridgeConfig {
            storage_path: "   ".to_string(),
            ..valid_config()
        };
        assert!(matches!(
            ClipSyncCore::start(config),
            Err(ClipSyncError::InvalidArgument)
        ));
    }

    #[test]
    fn invalid_discovery_target_is_rejected() {
        let config = BridgeConfig {
            discovery_targets: vec!["not-an-address".to_string()],
            ..valid_config()
        };
        assert!(matches!(
            ClipSyncCore::start(config),
            Err(ClipSyncError::InvalidArgument)
        ));
    }

    #[test]
    fn zero_port_is_rejected() {
        let config = BridgeConfig {
            listen_port: 0,
            ..valid_config()
        };
        assert!(matches!(
            ClipSyncCore::start(config),
            Err(ClipSyncError::InvalidArgument)
        ));
    }

    #[test]
    fn start_identity_stop_roundtrip() {
        let core = ClipSyncCore::start(valid_config()).expect("core must start");

        let identity = core.identity().expect("identity must be available");
        assert!(uuid::Uuid::parse_str(&identity.device_id).is_ok());
        assert!(!identity.device_name.is_empty());

        core.stop();
        assert!(matches!(core.identity(), Err(ClipSyncError::CoreStopped)));

        core.stop();
    }
}
