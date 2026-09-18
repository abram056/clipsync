use std::net::SocketAddr;
use std::path::PathBuf;

use clipboard_proto::error::{Error, Result};

const DEFAULT_DISCOVERY_PORT: u16 = 48271;
const DEFAULT_WS_PORT: u16 = 48272;
const DEFAULT_DISCOVERY_INTERVAL_SECS: u64 = 5;
const DEFAULT_HEARTBEAT_INTERVAL_SECS: u64 = 15;
const DEFAULT_PEER_TIMEOUT_SECS: u64 = 45;
const DEFAULT_RECONNECT_BACKOFF_INITIAL_MS: u64 = 1000;
const DEFAULT_RECONNECT_BACKOFF_MAX_MS: u64 = 60_000;
const DEFAULT_MAX_CLIPBOARD_BYTES: usize = 2_097_152;
const DEFAULT_MAX_PEERS: usize = 16;
const DEFAULT_REPLAY_CACHE_CAPACITY: usize = 1000;
const DEFAULT_REPLAY_CACHE_TTL_SECS: u64 = 3600;
const DEFAULT_PAIRING_TIMEOUT_SECS: u64 = 60;
const DEFAULT_CLIPBOARD_POLL_INTERVAL_MS: u64 = 250;
const DEFAULT_HISTORY_MAX_SIZE_BYTES: u64 = 2_097_152;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AppConfig {
    pub network: NetworkConfig,
    pub history: HistoryConfig,
    pub sync: SyncConfig,
    pub storage: StorageConfig,
    pub platform: PlatformConfig,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct NetworkConfig {
    pub discovery_port: u16,
    pub listen_port: u16,
    pub discovery_interval_secs: u64,
    pub heartbeat_interval_secs: u64,
    pub peer_timeout_secs: u64,
    pub reconnect_backoff_initial_ms: u64,
    pub reconnect_backoff_max_ms: u64,
    #[serde(default)]
    pub discovery_targets: Vec<SocketAddr>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct HistoryConfig {
    pub max_size_bytes: u64,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SyncConfig {
    pub max_clipboard_bytes: usize,
    pub max_peers: usize,
    pub replay_cache_capacity: usize,
    pub replay_cache_ttl_secs: u64,
    pub pairing_timeout_secs: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StorageConfig {
    pub path: PathBuf,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PlatformConfig {
    pub clipboard_poll_interval_ms: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("clipboard-sync");

        Self {
            network: NetworkConfig {
                discovery_port: DEFAULT_DISCOVERY_PORT,
                listen_port: DEFAULT_WS_PORT,
                discovery_interval_secs: DEFAULT_DISCOVERY_INTERVAL_SECS,
                heartbeat_interval_secs: DEFAULT_HEARTBEAT_INTERVAL_SECS,
                peer_timeout_secs: DEFAULT_PEER_TIMEOUT_SECS,
                reconnect_backoff_initial_ms: DEFAULT_RECONNECT_BACKOFF_INITIAL_MS,
                reconnect_backoff_max_ms: DEFAULT_RECONNECT_BACKOFF_MAX_MS,
                discovery_targets: Vec::new(),
            },
            history: HistoryConfig {
                max_size_bytes: DEFAULT_HISTORY_MAX_SIZE_BYTES,
            },
            sync: SyncConfig {
                max_clipboard_bytes: DEFAULT_MAX_CLIPBOARD_BYTES,
                max_peers: DEFAULT_MAX_PEERS,
                replay_cache_capacity: DEFAULT_REPLAY_CACHE_CAPACITY,
                replay_cache_ttl_secs: DEFAULT_REPLAY_CACHE_TTL_SECS,
                pairing_timeout_secs: DEFAULT_PAIRING_TIMEOUT_SECS,
                enabled: true,
            },
            storage: StorageConfig {
                path: data_dir.join("clipboard.db"),
            },
            platform: PlatformConfig {
                clipboard_poll_interval_ms: DEFAULT_CLIPBOARD_POLL_INTERVAL_MS,
            },
        }
    }
}

impl AppConfig {
    /// Load config from the default TOML path, overlaying defaults.
    pub fn load() -> Result<Self> {
        let config_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("clipboard-sync")
            .join("config.toml");

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| Error::Config(format!("failed to read config: {}", e)))?;
            let file_config: FileConfig = toml::from_str(&content)
                .map_err(|e| Error::Config(format!("failed to parse config: {}", e)))?;
            Ok(file_config.into())
        } else {
            Ok(AppConfig::default())
        }
    }
}

/// Partial config loaded from the TOML file. Fields are optional.
#[derive(Debug, Clone, serde::Deserialize)]
struct FileConfig {
    network: Option<NetworkConfig>,
    history: Option<HistoryConfig>,
    sync: Option<SyncConfig>,
    storage: Option<StorageConfig>,
    platform: Option<PlatformConfig>,
}

impl From<FileConfig> for AppConfig {
    fn from(fc: FileConfig) -> Self {
        let mut config = AppConfig::default();
        if let Some(n) = fc.network {
            config.network = n;
        }
        if let Some(h) = fc.history {
            config.history = h;
        }
        if let Some(s) = fc.sync {
            config.sync = s;
        }
        if let Some(st) = fc.storage {
            config.storage = st;
        }
        if let Some(p) = fc.platform {
            config.platform = p;
        }
        config
    }
}
