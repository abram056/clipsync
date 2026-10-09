use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ClipSyncError {
    #[error("invalid argument")]
    InvalidArgument,
    #[error("core is not running")]
    CoreNotRunning,
    #[error("core was stopped")]
    CoreStopped,
    #[error("storage failure")]
    StorageError,
    #[error("operation failed")]
    OperationFailed,
    #[error("invalid device id")]
    InvalidDeviceId,
    #[error("not found")]
    NotFound,
    #[error("internal error")]
    InternalError,
}

impl From<clipboard_proto::error::Error> for ClipSyncError {
    fn from(error: clipboard_proto::error::Error) -> Self {
        use clipboard_proto::error::Error as CoreError;
        match error {
            CoreError::Config(_) => ClipSyncError::InvalidArgument,
            CoreError::Storage(_) => ClipSyncError::StorageError,
            CoreError::Io(_) => ClipSyncError::InternalError,
            CoreError::Json(_) | CoreError::Protocol(_) | CoreError::ProtocolWithContext { .. } => {
                ClipSyncError::OperationFailed
            }
        }
    }
}
