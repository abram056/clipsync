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

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct AppConfig {
    pub network: NetworkConfig,
    pub history: HistoryConfig,
    pub sync: SyncConfig,
    pub storage: StorageConfig,
    pub platform: PlatformConfig,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub discovery_port: u16,
    pub listen_port: u16,
    pub discovery_interval_secs: u64,
    pub heartbeat_interval_secs: u64,
    pub peer_timeout_secs: u64,
    pub reconnect_backoff_initial_ms: u64,
    pub reconnect_backoff_max_ms: u64,
    pub discovery_targets: Vec<SocketAddr>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct HistoryConfig {
    pub max_size_bytes: u64,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct SyncConfig {
    pub max_clipboard_bytes: usize,
    pub max_peers: usize,
    pub replay_cache_capacity: usize,
    pub replay_cache_ttl_secs: u64,
    pub pairing_timeout_secs: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct StorageConfig {
    pub path: PathBuf,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct PlatformConfig {
    pub clipboard_poll_interval_ms: u64,
}

fn default_data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."))
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            discovery_port: DEFAULT_DISCOVERY_PORT,
            listen_port: DEFAULT_WS_PORT,
            discovery_interval_secs: DEFAULT_DISCOVERY_INTERVAL_SECS,
            heartbeat_interval_secs: DEFAULT_HEARTBEAT_INTERVAL_SECS,
            peer_timeout_secs: DEFAULT_PEER_TIMEOUT_SECS,
            reconnect_backoff_initial_ms: DEFAULT_RECONNECT_BACKOFF_INITIAL_MS,
            reconnect_backoff_max_ms: DEFAULT_RECONNECT_BACKOFF_MAX_MS,
            discovery_targets: Vec::new(),
        }
    }
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: DEFAULT_HISTORY_MAX_SIZE_BYTES,
        }
    }
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            max_clipboard_bytes: DEFAULT_MAX_CLIPBOARD_BYTES,
            max_peers: DEFAULT_MAX_PEERS,
            replay_cache_capacity: DEFAULT_REPLAY_CACHE_CAPACITY,
            replay_cache_ttl_secs: DEFAULT_REPLAY_CACHE_TTL_SECS,
            pairing_timeout_secs: DEFAULT_PAIRING_TIMEOUT_SECS,
            enabled: true,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: default_data_dir()
                .join("clipboard-sync")
                .join("clipboard.db"),
        }
    }
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            clipboard_poll_interval_ms: DEFAULT_CLIPBOARD_POLL_INTERVAL_MS,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Defaults match the constants documented in doc 08.
    #[test]
    fn defaults_match_documented_constants() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.network.discovery_port, 48271);
        assert_eq!(cfg.network.listen_port, 48272);
        assert_eq!(cfg.network.discovery_interval_secs, 5);
        assert_eq!(cfg.network.heartbeat_interval_secs, 15);
        assert_eq!(cfg.network.peer_timeout_secs, 45);
        assert_eq!(cfg.network.reconnect_backoff_initial_ms, 1000);
        assert_eq!(cfg.network.reconnect_backoff_max_ms, 60_000);
        assert!(cfg.network.discovery_targets.is_empty());
        assert_eq!(cfg.history.max_size_bytes, 2_097_152);
        assert_eq!(cfg.sync.max_clipboard_bytes, 2_097_152);
        assert_eq!(cfg.sync.max_peers, 16);
        assert_eq!(cfg.sync.replay_cache_capacity, 1000);
        assert_eq!(cfg.sync.replay_cache_ttl_secs, 3600);
        assert_eq!(cfg.sync.pairing_timeout_secs, 60);
        assert!(cfg.sync.enabled);
        assert_eq!(cfg.platform.clipboard_poll_interval_ms, 250);
    }

    /// A complete file replaces every section with its file values.
    #[test]
    fn full_file_overlays_every_section() {
        let toml = r#"
            [network]
            discovery_port = 12341
            listen_port = 12342
            discovery_interval_secs = 2
            heartbeat_interval_secs = 7
            peer_timeout_secs = 31
            reconnect_backoff_initial_ms = 250
            reconnect_backoff_max_ms = 5000
            discovery_targets = ["127.0.0.1:9999"]

            [history]
            max_size_bytes = 4096

            [sync]
            max_clipboard_bytes = 1024
            max_peers = 3
            replay_cache_capacity = 7
            replay_cache_ttl_secs = 11
            pairing_timeout_secs = 9
            enabled = false

            [storage]
            path = "/tmp/clipsync-config-test.db"

            [platform]
            clipboard_poll_interval_ms = 100
        "#;
        let fc: FileConfig = toml::from_str(toml).expect("complete file must parse");
        let cfg = AppConfig::from(fc);

        assert_eq!(cfg.network.discovery_port, 12341);
        assert_eq!(cfg.network.listen_port, 12342);
        assert_eq!(cfg.network.discovery_interval_secs, 2);
        assert_eq!(cfg.network.heartbeat_interval_secs, 7);
        assert_eq!(cfg.network.peer_timeout_secs, 31);
        assert_eq!(cfg.network.reconnect_backoff_initial_ms, 250);
        assert_eq!(cfg.network.reconnect_backoff_max_ms, 5000);
        assert_eq!(
            cfg.network.discovery_targets,
            vec!["127.0.0.1:9999".parse::<SocketAddr>().unwrap()]
        );
        assert_eq!(cfg.history.max_size_bytes, 4096);
        assert_eq!(cfg.sync.max_clipboard_bytes, 1024);
        assert_eq!(cfg.sync.max_peers, 3);
        assert_eq!(cfg.sync.replay_cache_capacity, 7);
        assert_eq!(cfg.sync.replay_cache_ttl_secs, 11);
        assert_eq!(cfg.sync.pairing_timeout_secs, 9);
        assert!(!cfg.sync.enabled);
        assert_eq!(
            cfg.storage.path,
            PathBuf::from("/tmp/clipsync-config-test.db")
        );
        assert_eq!(cfg.platform.clipboard_poll_interval_ms, 100);
    }

    /// An empty (or absent-equivalent) file yields exactly the defaults.
    #[test]
    fn empty_file_yields_defaults() {
        let fc: FileConfig = toml::from_str("").expect("empty file must parse");
        let cfg = AppConfig::from(fc);
        assert_eq!(
            format!("{:?}", cfg),
            format!("{:?}", AppConfig::default()),
            "an empty file must overlay nothing"
        );
    }

    /// Structurally broken TOML is rejected rather than silently defaulted.
    #[test]
    fn malformed_toml_is_rejected() {
        assert!(toml::from_str::<FileConfig>("[network\nlisten_port = 1").is_err());
    }

    /// A value of the wrong type is rejected (invalid-value rejection per
    /// doc 10's config coverage row).
    #[test]
    fn invalid_value_is_rejected() {
        assert!(
            toml::from_str::<FileConfig>("[network]\ndiscovery_port = \"not-a-port\"").is_err()
        );
        assert!(toml::from_str::<FileConfig>("[sync]\nenabled = 17").is_err());
    }

    /// Doc 08: "Only keys the user sets need to appear in the file; unset
    /// keys fall back to defaults." Section-level `#[serde(default)]` fills
    /// unset keys from the section's documented defaults.
    #[test]
    fn partial_section_overlay_keeps_unmentioned_defaults() {
        let toml = "[network]\nlisten_port = 12342\n";
        let fc: FileConfig = toml::from_str(toml).expect("a partial section must parse");
        let cfg = AppConfig::from(fc);

        assert_eq!(cfg.network.listen_port, 12342, "the key set must apply");
        assert_eq!(
            cfg.network.discovery_port, 48271,
            "keys the user did not set must fall back to defaults"
        );
        assert_eq!(cfg.network.heartbeat_interval_secs, 15);
        assert_eq!(
            cfg.sync.pairing_timeout_secs, 60,
            "unmentioned sections too"
        );
    }
}
