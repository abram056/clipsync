use std::net::SocketAddr;
use std::path::PathBuf;

use clipboard_proto::event::{Event, EventType};
use clipboard_proto::types::{DeviceInfo, HistoryEntry, PairingRequest, Platform, TrustedPeer};

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

fn platform_to_string(platform: Platform) -> String {
    platform.to_string()
}

fn datetime_to_millis(dt: chrono::DateTime<chrono::Utc>) -> i64 {
    dt.timestamp_millis()
}

/// UI-facing device representation crossing the Android boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeDevice {
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub is_trusted: bool,
    pub is_connected: bool,
    pub last_seen_ms: Option<i64>,
}

impl BridgeDevice {
    pub fn from_trusted(peer: &TrustedPeer, is_connected: bool) -> Self {
        Self {
            device_id: peer.device_id.to_string(),
            device_name: peer.device_name.clone(),
            platform: peer
                .platform
                .map(platform_to_string)
                .unwrap_or_else(|| "unknown".to_string()),
            is_trusted: true,
            is_connected,
            last_seen_ms: peer.last_seen.map(datetime_to_millis),
        }
    }

    pub fn from_discovered(info: &DeviceInfo) -> Self {
        Self {
            device_id: info.device_id.to_string(),
            device_name: info.device_name.clone(),
            platform: platform_to_string(info.platform),
            is_trusted: info.is_trusted,
            is_connected: info.is_connected,
            last_seen_ms: None,
        }
    }
}

/// UI-facing pending pairing request.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgePairingRequest {
    pub request_id: String,
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub received_at_ms: i64,
}

impl From<&PairingRequest> for BridgePairingRequest {
    fn from(req: &PairingRequest) -> Self {
        Self {
            request_id: req.request_id.to_string(),
            device_id: req.device_id.to_string(),
            device_name: req.device_name.clone(),
            platform: platform_to_string(req.platform),
            received_at_ms: datetime_to_millis(req.received_at),
        }
    }
}

/// UI-facing clipboard history entry.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeHistoryEntry {
    pub clipboard_id: String,
    pub content: String,
    pub content_type: String,
    pub content_hash: String,
    pub origin_device_id: String,
    pub created_at_ms: i64,
    pub size_bytes: i64,
}

impl From<&HistoryEntry> for BridgeHistoryEntry {
    fn from(entry: &HistoryEntry) -> Self {
        Self {
            clipboard_id: entry.clipboard_id.to_string(),
            content: entry.content.clone(),
            content_type: entry.content_type.clone(),
            content_hash: entry.content_hash.clone(),
            origin_device_id: entry.origin_device_id.to_string(),
            created_at_ms: datetime_to_millis(entry.created_at),
            size_bytes: entry.size_bytes as i64,
        }
    }
}

/// Curated event kinds exposed to Android.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeEventKind {
    ApplicationStarted,
    ApplicationStopping,
    DeviceDiscovered,
    DeviceConnected,
    DeviceDisconnected,
    DeviceConnectionFailed,
    PairingRequested,
    PairingAccepted,
    PairingRejected,
    PairingFailed,
    ClipboardUpdatedFromRemote,
    ClipboardHistoryUpdated,
    ClipboardRestored,
    SyncCompleted,
    DuplicateClipboardIgnored,
    NetworkError,
    ProtocolError,
    EventStreamLagged,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceEventPayload {
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardEventPayload {
    pub clipboard_id: String,
    pub content: String,
    pub content_type: String,
    pub content_hash: String,
    pub origin_device_id: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncEventPayload {
    pub peer_count: i64,
    pub history_bytes: i64,
    pub reason: String,
}

/// Bridge-owned event DTO crossing the Android boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub kind: BridgeEventKind,
    pub device: Option<DeviceEventPayload>,
    pub clipboard: Option<ClipboardEventPayload>,
    pub sync: Option<SyncEventPayload>,
    pub dropped_events: i64,
}

impl BridgeEvent {
    fn new(kind: BridgeEventKind, timestamp_ms: i64) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            timestamp_ms,
            kind,
            device: None,
            clipboard: None,
            sync: None,
            dropped_events: 0,
        }
    }

    fn with_device(
        mut self,
        device_id: uuid::Uuid,
        name: Option<&str>,
        platform: Option<Platform>,
        reason: &str,
    ) -> Self {
        self.device = Some(DeviceEventPayload {
            device_id: device_id.to_string(),
            device_name: name.unwrap_or_default().to_string(),
            platform: platform.map(platform_to_string).unwrap_or_default(),
            reason: reason.to_string(),
        });
        self
    }

    fn with_clipboard(
        mut self,
        clipboard_id: uuid::Uuid,
        content: &str,
        content_type: &str,
        content_hash: &str,
        origin: uuid::Uuid,
        created_at_ms: i64,
    ) -> Self {
        self.clipboard = Some(ClipboardEventPayload {
            clipboard_id: clipboard_id.to_string(),
            content: content.to_string(),
            content_type: content_type.to_string(),
            content_hash: content_hash.to_string(),
            origin_device_id: origin.to_string(),
            created_at_ms,
        });
        self
    }

    fn with_sync(mut self, peer_count: i64, history_bytes: i64, reason: &str) -> Self {
        self.sync = Some(SyncEventPayload {
            peer_count,
            history_bytes,
            reason: reason.to_string(),
        });
        self
    }

    fn with_dropped(mut self, dropped: i64) -> Self {
        self.dropped_events = dropped;
        self
    }
}

fn disconnect_reason_str(reason: &clipboard_proto::event::DisconnectReason) -> &'static str {
    use clipboard_proto::event::DisconnectReason;
    match reason {
        DisconnectReason::Graceful => "graceful",
        DisconnectReason::Error => "error",
        DisconnectReason::Timeout => "timeout",
    }
}

fn pairing_reject_reason_str(reason: &clipboard_proto::event::PairingRejectReason) -> &'static str {
    use clipboard_proto::event::PairingRejectReason;
    match reason {
        PairingRejectReason::UserDenied => "user_denied",
        PairingRejectReason::Timeout => "timeout",
        PairingRejectReason::InternalError => "internal_error",
    }
}

fn error_code_str(code: clipboard_proto::error::ErrorCode) -> &'static str {
    use clipboard_proto::error::ErrorCode;
    match code {
        ErrorCode::UnknownError => "unknown",
        ErrorCode::ProtocolVersionUnsupported => "protocol_version_unsupported",
        ErrorCode::UnknownDevice => "unknown_device",
        ErrorCode::AuthenticationFailed => "authentication_failed",
        ErrorCode::MalformedMessage => "malformed_message",
        ErrorCode::MissingField => "missing_field",
        ErrorCode::UnsupportedContentType => "unsupported_content_type",
        ErrorCode::PayloadTooLarge => "payload_too_large",
        ErrorCode::DuplicateMessage => "duplicate_message",
        ErrorCode::StaleMessage => "stale_message",
        ErrorCode::PairingDenied => "pairing_denied",
        ErrorCode::PairingTimeout => "pairing_timeout",
        ErrorCode::ConnectionLimit => "connection_limit",
        ErrorCode::InternalError => "internal_error",
    }
}

/// Convert an internal core event into a curated bridge event.
/// Returns `None` for events that should remain internal.
pub fn event_to_bridge(event: &Event) -> Option<BridgeEvent> {
    let ts = event.timestamp.timestamp_millis();
    let bridge = match &event.event_type {
        EventType::ApplicationStarted(_) => {
            BridgeEvent::new(BridgeEventKind::ApplicationStarted, ts)
        }
        EventType::ApplicationStopping(_) => {
            BridgeEvent::new(BridgeEventKind::ApplicationStopping, ts)
        }
        EventType::DeviceDiscovered(p) => BridgeEvent::new(BridgeEventKind::DeviceDiscovered, ts)
            .with_device(p.device_id, Some(&p.device_name), Some(p.platform), ""),
        EventType::DeviceConnected(p) => BridgeEvent::new(BridgeEventKind::DeviceConnected, ts)
            .with_device(p.device_id, Some(&p.device_name), None, ""),
        EventType::DeviceDisconnected(p) => BridgeEvent::new(
            BridgeEventKind::DeviceDisconnected,
            ts,
        )
        .with_device(p.device_id, None, None, disconnect_reason_str(&p.reason)),
        EventType::DeviceConnectionFailed(p) => BridgeEvent::new(
            BridgeEventKind::DeviceConnectionFailed,
            ts,
        )
        .with_device(p.device_id, None, None, &p.error),
        EventType::PairingRequested(p) => BridgeEvent::new(BridgeEventKind::PairingRequested, ts)
            .with_device(p.device_id, Some(&p.device_name), Some(p.platform), ""),
        EventType::PairingAccepted(p) => BridgeEvent::new(BridgeEventKind::PairingAccepted, ts)
            .with_device(p.device_id, None, None, ""),
        EventType::PairingRejected(p) => BridgeEvent::new(BridgeEventKind::PairingRejected, ts)
            .with_device(
                p.device_id,
                None,
                None,
                pairing_reject_reason_str(&p.reason),
            ),
        EventType::PairingFailed(p) => BridgeEvent::new(BridgeEventKind::PairingFailed, ts)
            .with_device(p.request_id, None, None, &p.error),
        EventType::ClipboardUpdatedFromRemote(p) => {
            BridgeEvent::new(BridgeEventKind::ClipboardUpdatedFromRemote, ts).with_clipboard(
                p.clipboard_id,
                &p.content,
                &p.content_type,
                &p.content_hash,
                p.origin_device_id,
                ts,
            )
        }
        EventType::ClipboardHistoryUpdated(p) => BridgeEvent::new(
            BridgeEventKind::ClipboardHistoryUpdated,
            ts,
        )
        .with_sync(0, p.current_history_size as i64, "history updated"),
        EventType::ClipboardRestored(p) => BridgeEvent::new(BridgeEventKind::ClipboardRestored, ts)
            .with_clipboard(p.clipboard_id, "", "", "", uuid::Uuid::nil(), ts),
        EventType::SyncCompleted(p) => BridgeEvent::new(BridgeEventKind::SyncCompleted, ts)
            .with_sync(
                p.successful_devices.len() as i64,
                p.failed_devices.len() as i64,
                "",
            ),
        EventType::DuplicateClipboardIgnored(_) => {
            BridgeEvent::new(BridgeEventKind::DuplicateClipboardIgnored, ts)
        }
        EventType::NetworkError(p) => BridgeEvent::new(BridgeEventKind::NetworkError, ts)
            .with_device(p.device_id, None, None, &p.error),
        EventType::ProtocolError(p) => BridgeEvent::new(BridgeEventKind::ProtocolError, ts)
            .with_device(p.device_id, None, None, error_code_str(p.error)),
        _ => return None,
    };
    Some(bridge)
}

/// Build a synthetic lag event describing dropped broadcasts.
pub fn lag_event(dropped: u64) -> BridgeEvent {
    BridgeEvent::new(
        BridgeEventKind::EventStreamLagged,
        chrono::Utc::now().timestamp_millis(),
    )
    .with_sync(0, 0, "event stream lagged")
    .with_dropped(dropped as i64)
}
