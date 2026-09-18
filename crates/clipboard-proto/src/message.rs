use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ErrorCode;

/// Protocol version currently supported.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum clipboard payload size in bytes (2 MB).
pub const MAX_CLIPBOARD_SIZE: usize = 2_097_152;

/// Content types supported in the MVP.
pub const SUPPORTED_CONTENT_TYPES: &[&str] = &["text/plain"];

// ---------------------------------------------------------------------------
// Message Envelope
// ---------------------------------------------------------------------------

/// The envelope wrapping every protocol message on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub message_id: Uuid,
    pub message_type: MessageType,
    pub device_id: Uuid,
    pub device_name: String,
    pub timestamp: DateTime<Utc>,
    pub payload: Payload,
}

impl Envelope {
    /// Build an envelope with a fresh UUIDv4 message ID and current timestamp.
    pub fn build(
        message_type: MessageType,
        device_id: Uuid,
        device_name: String,
        payload: Payload,
    ) -> Self {
        Self {
            message_id: Uuid::new_v4(),
            message_type,
            device_id,
            device_name,
            timestamp: Utc::now(),
            payload,
        }
    }

    /// Serialize to JSON bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize from JSON bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}

// ---------------------------------------------------------------------------
// Message Types
// ---------------------------------------------------------------------------

/// Identifies the kind of protocol message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MessageType {
    Discover,
    DiscoverResponse,
    Hello,
    HelloAck,
    Goodbye,
    PairingRequest,
    PairingAccept,
    PairingReject,
    ClipboardUpdate,
    ClipboardAck,
    Ping,
    Pong,
    Error,
}

// ---------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------

/// The payload of a protocol message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Payload {
    Discover(DiscoverPayload),
    DiscoverResponse(DiscoverResponsePayload),
    Hello(HelloPayload),
    HelloAck(HelloAckPayload),
    Goodbye(GoodbyePayload),
    PairingRequest(PairingRequestPayload),
    PairingAccept(PairingAcceptPayload),
    PairingReject(PairingRejectPayload),
    ClipboardUpdate(ClipboardUpdatePayload),
    ClipboardAck(ClipboardAckPayload),
    Ping,
    Pong,
    Error(ErrorPayload),
}

// --- Discovery ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverPayload {
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverResponsePayload {
    pub protocol_version: u32,
    pub listening_port: u16,
    pub platform: crate::types::Platform,
}

// --- Connection ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloPayload {
    pub protocol_version: u32,
    pub supported_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloAckPayload {
    pub accepted: bool,
    pub reason: Option<ErrorCode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoodbyePayload {
    pub reason: DisconnectReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisconnectReason {
    Graceful,
    Error,
    Timeout,
}

// --- Pairing ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequestPayload {
    pub device_id: Uuid,
    pub device_name: String,
    pub platform: crate::types::Platform,
    pub request_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingAcceptPayload {
    pub request_id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    pub paired_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRejectPayload {
    pub request_id: Uuid,
    pub reason: PairingRejectReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingRejectReason {
    UserDenied,
    Timeout,
    InternalError,
}

// --- Clipboard ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardUpdatePayload {
    pub clipboard_id: Uuid,
    pub origin_device_id: Uuid,
    pub content_type: String,
    pub content: String,
    pub content_hash: String,
    pub creation_timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardAckPayload {
    pub clipboard_id: Uuid,
    pub status: ClipboardAckStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClipboardAckStatus {
    Accepted,
    Rejected { code: ErrorCode },
}

// --- Error ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: ErrorCode,
    pub message: String,
    pub related_message_id: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// Platform helper
// ---------------------------------------------------------------------------

/// Detect the current platform.
pub fn current_platform() -> crate::types::Platform {
    if cfg!(target_os = "linux") {
        crate::types::Platform::Linux
    } else if cfg!(target_os = "android") {
        crate::types::Platform::Android
    } else if cfg!(target_os = "windows") {
        crate::types::Platform::Windows
    } else if cfg!(target_os = "macos") {
        crate::types::Platform::MacOS
    } else {
        crate::types::Platform::Linux
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip() {
        let id = Uuid::new_v4();
        let payload = Payload::Hello(HelloPayload {
            protocol_version: PROTOCOL_VERSION,
            supported_capabilities: vec!["text/plain".to_string()],
        });
        let env = Envelope {
            message_id: id,
            message_type: MessageType::Hello,
            device_id: Uuid::new_v4(),
            device_name: "test-device".to_string(),
            timestamp: Utc::now(),
            payload,
        };

        let json = env.to_bytes().unwrap();
        let restored = Envelope::from_bytes(&json).unwrap();

        assert_eq!(restored.message_id, id);
        assert_eq!(restored.message_type, MessageType::Hello);
        match restored.payload {
            Payload::Hello(h) => {
                assert_eq!(h.protocol_version, PROTOCOL_VERSION);
                assert_eq!(h.supported_capabilities, vec!["text/plain"]);
            }
            _ => panic!("wrong payload variant"),
        }
    }

    #[test]
    fn discover_roundtrip() {
        let env = Envelope::build(
            MessageType::Discover,
            Uuid::new_v4(),
            "node-a".to_string(),
            Payload::Discover(DiscoverPayload {
                protocol_version: PROTOCOL_VERSION,
            }),
        );
        let json = env.to_bytes().unwrap();
        let restored = Envelope::from_bytes(&json).unwrap();
        assert_eq!(restored.message_type, MessageType::Discover);
    }

    #[test]
    fn clipboard_update_roundtrip() {
        use sha2::Digest;
        let content_hash = sha2::Sha256::digest(b"hello");
        let hash_hex: String = content_hash.iter().map(|b| format!("{:02x}", b)).collect();
        let env = Envelope::build(
            MessageType::ClipboardUpdate,
            Uuid::new_v4(),
            "phone".to_string(),
            Payload::ClipboardUpdate(ClipboardUpdatePayload {
                clipboard_id: Uuid::new_v4(),
                origin_device_id: Uuid::new_v4(),
                content_type: "text/plain".to_string(),
                content: "hello".to_string(),
                content_hash: hash_hex.clone(),
                creation_timestamp: Utc::now(),
            }),
        );
        let json = env.to_bytes().unwrap();
        let restored = Envelope::from_bytes(&json).unwrap();
        if let Payload::ClipboardUpdate(cu) = restored.payload {
            assert_eq!(cu.content_hash, hash_hex);
            assert_eq!(cu.content, "hello");
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn clipboard_ack_status_roundtrip() {
        let accepted = ClipboardAckStatus::Accepted;
        let json = serde_json::to_string(&accepted).unwrap();
        let restored: ClipboardAckStatus = serde_json::from_str(&json).unwrap();
        assert!(matches!(restored, ClipboardAckStatus::Accepted));

        let rejected = ClipboardAckStatus::Rejected {
            code: ErrorCode::PayloadTooLarge,
        };
        let json = serde_json::to_string(&rejected).unwrap();
        let restored: ClipboardAckStatus = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            restored,
            ClipboardAckStatus::Rejected {
                code: ErrorCode::PayloadTooLarge
            }
        ));
    }
}
