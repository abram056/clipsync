use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ErrorCode;
use crate::types::Platform;

// ---------------------------------------------------------------------------
// Event Model
// ---------------------------------------------------------------------------

/// An internal event exchanged between application components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub source: EventSource,
    pub event_type: EventType,
}

impl Event {
    pub fn new(source: EventSource, event_type: EventType) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            source,
            event_type,
        }
    }
}

// ---------------------------------------------------------------------------
// Event Sources
// ---------------------------------------------------------------------------

/// The component that emitted an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum EventSource {
    ClipboardMonitor,
    SyncEngine,
    DiscoveryService,
    MessagingService,
    HistoryManager,
    PairingManager,
    UserInterface,
}

// ---------------------------------------------------------------------------
// Event Types
// ---------------------------------------------------------------------------

/// The type and payload of an internal event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventType {
    // Application lifecycle
    ApplicationStarted(ApplicationStartedPayload),
    ApplicationStopping(ApplicationStoppingPayload),

    // Clipboard events
    ClipboardChanged(ClipboardChangedPayload),
    ClipboardUpdatedFromRemote(ClipboardUpdatedFromRemotePayload),
    ClipboardHistoryUpdated(ClipboardHistoryUpdatedPayload),
    ClipboardRestoreRequested(ClipboardRestoreRequestedPayload),
    ClipboardRestored(ClipboardRestoredPayload),

    // Discovery events
    DeviceDiscovered(DeviceDiscoveredPayload),
    DeviceConnected(DeviceConnectedPayload),
    DeviceDisconnected(DeviceDisconnectedPayload),
    DeviceConnectionFailed(DeviceConnectionFailedPayload),
    PeerListUpdated(PeerListUpdatedPayload),

    // Sync events
    SyncRequested(SyncRequestedPayload),
    SyncCompleted(SyncCompletedPayload),
    DuplicateClipboardIgnored(DuplicateClipboardIgnoredPayload),

    // Pairing events
    PairingRequested(PairingRequestedPayload),
    PairingAccepted(PairingAcceptedPayload),
    PairingRejected(PairingRejectedPayload),
    PairingFailed(PairingFailedPayload),

    // Error events
    NetworkError(NetworkErrorPayload),
    ProtocolError(ProtocolErrorPayload),
}

// ---------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------

// --- Application lifecycle ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationStartedPayload {
    pub version: String,
    pub device_id: Uuid,
    pub device_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationStoppingPayload {
    pub reason: ShutdownReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShutdownReason {
    Graceful,
    User,
    Error,
}

// --- Clipboard events ---

/// Emitted when the local OS clipboard changes.
/// The monitor carries only content; the Sync Engine assigns clipboard_id and hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardChangedPayload {
    pub content: String,
    pub content_type: String,
}

/// Emitted after receiving clipboard data from a remote device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardUpdatedFromRemotePayload {
    pub clipboard_id: Uuid,
    pub origin_device_id: Uuid,
    pub content: String,
    pub content_type: String,
    pub content_hash: String,
}

/// Emitted when clipboard history is modified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardHistoryUpdatedPayload {
    pub clipboard_id: Uuid,
    pub current_history_size: u64,
}

/// Emitted when the user requests restoration of a history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardRestoreRequestedPayload {
    pub clipboard_id: Uuid,
}

/// Emitted when restoration is complete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardRestoredPayload {
    pub clipboard_id: Uuid,
}

// --- Discovery events ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDiscoveredPayload {
    pub device_id: Uuid,
    pub device_name: String,
    pub ip_address: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConnectedPayload {
    pub device_id: Uuid,
    pub device_name: String,
    pub connection_type: ConnectionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionType {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDisconnectedPayload {
    pub device_id: Uuid,
    pub reason: DisconnectReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisconnectReason {
    Graceful,
    Error,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConnectionFailedPayload {
    pub device_id: Uuid,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerListUpdatedPayload {
    pub connected_devices: Vec<crate::types::DeviceInfo>,
}

// --- Sync events ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequestedPayload {
    pub clipboard_id: Uuid,
    pub target_devices: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCompletedPayload {
    pub clipboard_id: Uuid,
    pub successful_devices: Vec<Uuid>,
    pub failed_devices: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateClipboardIgnoredPayload {
    pub clipboard_id: Uuid,
    pub reason: DuplicateReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateReason {
    DuplicateContent,
    SameOrigin,
}

// --- Pairing events ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequestedPayload {
    pub request_id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    pub platform: Platform,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingAcceptedPayload {
    pub request_id: Uuid,
    pub device_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRejectedPayload {
    pub request_id: Uuid,
    pub device_id: Uuid,
    pub reason: PairingRejectReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingRejectReason {
    UserDenied,
    Timeout,
    InternalError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingFailedPayload {
    pub request_id: Uuid,
    pub error: String,
}

// --- Error events ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkErrorPayload {
    pub device_id: Uuid,
    pub operation: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolErrorPayload {
    pub device_id: Uuid,
    pub message_id: Uuid,
    pub error: ErrorCode,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_roundtrip() {
        let event = Event::new(
            EventSource::SyncEngine,
            EventType::ClipboardChanged(ClipboardChangedPayload {
                content: "hello world".to_string(),
                content_type: "text/plain".to_string(),
            }),
        );

        let json = serde_json::to_string(&event).unwrap();
        let restored: Event = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.source, EventSource::SyncEngine);
        match restored.event_type {
            EventType::ClipboardChanged(p) => {
                assert_eq!(p.content, "hello world");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn event_source_serialization() {
        let sources = [
            EventSource::ClipboardMonitor,
            EventSource::SyncEngine,
            EventSource::DiscoveryService,
            EventSource::MessagingService,
            EventSource::HistoryManager,
            EventSource::PairingManager,
            EventSource::UserInterface,
        ];
        for source in &sources {
            let json = serde_json::to_string(source).unwrap();
            let restored: EventSource = serde_json::from_str(&json).unwrap();
            assert_eq!(&restored, source);
        }
    }

    #[test]
    fn pairing_event_roundtrip() {
        let event = Event::new(
            EventSource::PairingManager,
            EventType::PairingRequested(PairingRequestedPayload {
                request_id: Uuid::new_v4(),
                device_id: Uuid::new_v4(),
                device_name: "phone".to_string(),
                platform: Platform::Android,
            }),
        );

        let json = serde_json::to_string(&event).unwrap();
        let restored: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.source, EventSource::PairingManager);
    }
}
