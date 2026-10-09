use std::net::SocketAddr;
use std::path::PathBuf;

use crate::config::AppConfig;

use super::ClipSyncError;

pub struct BridgeConfig {
    pub storage_path: String,
    pub discovery_port: u16,
    pub listen_port: u16,
    pub discovery_targets: Vec<String>,
    pub sync_enabled: bool,
}

impl BridgeConfig {
    pub fn into_app_config(self) -> Result<AppConfig, ClipSyncError> {
        let BridgeConfig {
            storage_path,
            discovery_port,
            listen_port,
            discovery_targets,
            sync_enabled,
        } = self;

        if storage_path.trim().is_empty() {
            return Err(ClipSyncError::InvalidArgument);
        }
        if discovery_port == 0 || listen_port == 0 {
            return Err(ClipSyncError::InvalidArgument);
        }

        let mut targets = Vec::with_capacity(discovery_targets.len());
        for target in &discovery_targets {
            match target.parse::<SocketAddr>() {
                Ok(addr) => targets.push(addr),
                Err(_) => return Err(ClipSyncError::InvalidArgument),
            }
        }

        let mut config = AppConfig::default();
        config.storage.path = PathBuf::from(storage_path);
        config.network.discovery_port = discovery_port;
        config.network.listen_port = listen_port;
        config.network.discovery_targets = targets;
        config.sync.enabled = sync_enabled;
        Ok(config)
    }
}

#[derive(Debug)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub device_name: String,
}
