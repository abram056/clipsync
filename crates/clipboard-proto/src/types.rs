use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Supported platform values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    Android,
    Windows,
    MacOS,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Linux => write!(f, "linux"),
            Platform::Android => write!(f, "android"),
            Platform::Windows => write!(f, "windows"),
            Platform::MacOS => write!(f, "macos"),
        }
    }
}

/// Device information shared across the UniFFI boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: uuid::Uuid,
    pub device_name: String,
    pub platform: Platform,
    pub is_trusted: bool,
    pub is_connected: bool,
}

/// A pending pairing request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequest {
    pub request_id: uuid::Uuid,
    pub device_id: uuid::Uuid,
    pub device_name: String,
    pub platform: Platform,
    pub received_at: DateTime<Utc>,
}

/// Synchronization status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SyncStatus {
    Active,
    Paused,
    Syncing {
        peer_count: usize,
        history_bytes: u64,
    },
}

/// A clipboard history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub clipboard_id: uuid::Uuid,
    pub content: String,
    pub content_type: String,
    pub content_hash: String,
    pub origin_device_id: uuid::Uuid,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
}

/// Information about a peer device needed for connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub device_id: uuid::Uuid,
    pub device_name: String,
    pub platform: Platform,
    pub address: SocketAddr,
}

/// Device identity persisted in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub device_id: uuid::Uuid,
    pub device_name: String,
    pub created_at: DateTime<Utc>,
}

/// Trusted peer information persisted in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub device_id: uuid::Uuid,
    pub device_name: String,
    pub platform: Option<Platform>,
    pub paired_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
}
