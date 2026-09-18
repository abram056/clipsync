use serde::{Deserialize, Serialize};
use std::fmt;

/// Error codes for the Clipboard Sync protocol.
/// These match the wire-level error codes defined in the protocol specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ErrorCode {
    /// Unclassified failure.
    UnknownError = 0,
    /// Peer uses an unsupported protocol version.
    ProtocolVersionUnsupported = 1,
    /// Sender is not a trusted device.
    UnknownDevice = 2,
    /// HELLO handshake validation failed.
    AuthenticationFailed = 3,
    /// Message failed structural validation.
    MalformedMessage = 4,
    /// A required field is absent.
    MissingField = 5,
    /// Unsupported clipboard content type.
    UnsupportedContentType = 6,
    /// Payload exceeds the maximum clipboard size.
    PayloadTooLarge = 7,
    /// Message ID was already processed (replay).
    DuplicateMessage = 8,
    /// Message timestamp is outside the acceptable window.
    StaleMessage = 9,
    /// Pairing request was rejected by the user.
    PairingDenied = 10,
    /// Pairing request was not answered in time.
    PairingTimeout = 11,
    /// Peer connection limit was exceeded.
    ConnectionLimit = 12,
    /// Unexpected internal failure.
    InternalError = 13,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCode::UnknownError => write!(f, "UNKNOWN_ERROR"),
            ErrorCode::ProtocolVersionUnsupported => write!(f, "PROTOCOL_VERSION_UNSUPPORTED"),
            ErrorCode::UnknownDevice => write!(f, "UNKNOWN_DEVICE"),
            ErrorCode::AuthenticationFailed => write!(f, "AUTHENTICATION_FAILED"),
            ErrorCode::MalformedMessage => write!(f, "MALFORMED_MESSAGE"),
            ErrorCode::MissingField => write!(f, "MISSING_FIELD"),
            ErrorCode::UnsupportedContentType => write!(f, "UNSUPPORTED_CONTENT_TYPE"),
            ErrorCode::PayloadTooLarge => write!(f, "PAYLOAD_TOO_LARGE"),
            ErrorCode::DuplicateMessage => write!(f, "DUPLICATE_MESSAGE"),
            ErrorCode::StaleMessage => write!(f, "STALE_MESSAGE"),
            ErrorCode::PairingDenied => write!(f, "PAIRING_DENIED"),
            ErrorCode::PairingTimeout => write!(f, "PAIRING_TIMEOUT"),
            ErrorCode::ConnectionLimit => write!(f, "CONNECTION_LIMIT"),
            ErrorCode::InternalError => write!(f, "INTERNAL_ERROR"),
        }
    }
}

impl TryFrom<u8> for ErrorCode {
    type Error = ();

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(ErrorCode::UnknownError),
            1 => Ok(ErrorCode::ProtocolVersionUnsupported),
            2 => Ok(ErrorCode::UnknownDevice),
            3 => Ok(ErrorCode::AuthenticationFailed),
            4 => Ok(ErrorCode::MalformedMessage),
            5 => Ok(ErrorCode::MissingField),
            6 => Ok(ErrorCode::UnsupportedContentType),
            7 => Ok(ErrorCode::PayloadTooLarge),
            8 => Ok(ErrorCode::DuplicateMessage),
            9 => Ok(ErrorCode::StaleMessage),
            10 => Ok(ErrorCode::PairingDenied),
            11 => Ok(ErrorCode::PairingTimeout),
            12 => Ok(ErrorCode::ConnectionLimit),
            13 => Ok(ErrorCode::InternalError),
            _ => Err(()),
        }
    }
}

/// Application error type wrapping protocol error codes.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Protocol error: {0}")]
    Protocol(ErrorCode),

    #[error("Protocol error: {code} — {message}")]
    ProtocolWithContext { code: ErrorCode, message: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_roundtrip() {
        for code in 0u8..=13 {
            let ec = ErrorCode::try_from(code).unwrap();
            assert_eq!(ec as u8, code);
        }
        assert!(ErrorCode::try_from(14u8).is_err());
    }

    #[test]
    fn error_code_display() {
        assert_eq!(ErrorCode::UnknownError.to_string(), "UNKNOWN_ERROR");
        assert_eq!(
            ErrorCode::ProtocolVersionUnsupported.to_string(),
            "PROTOCOL_VERSION_UNSUPPORTED"
        );
        assert_eq!(ErrorCode::PayloadTooLarge.to_string(), "PAYLOAD_TOO_LARGE");
    }
}
