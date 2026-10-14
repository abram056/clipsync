package com.abram056.clipsync.data

data class Device(
    val deviceId: String,
    val deviceName: String,
    val platform: String,
    val isTrusted: Boolean,
    val isConnected: Boolean,
    val lastSeenMs: Long?,
)

data class PairingRequest(
    val requestId: String,
    val deviceId: String,
    val deviceName: String,
    val platform: String,
    val receivedAtMs: Long,
)

data class HistoryItem(
    val clipboardId: String,
    val content: String,
    val contentType: String,
    val contentHash: String,
    val originDeviceId: String,
    val createdAtMs: Long,
    val sizeBytes: Long,
)

data class SyncState(
    val isPaused: Boolean = false,
    val peerCount: Int = 0,
    val historyBytes: Long = 0L,
)

/// This device's stable identity as reported by the core (doc 12 §30).
data class Identity(
    val deviceId: String,
    val deviceName: String,
)

enum class RepositoryErrorSeverity {
    /// Expected, non-blocking feedback (a one-shot action result, a transient network blip).
    TRANSIENT,

    /// A condition that needs the user's attention until it is resolved.
    PERSISTENT,
}

/// Coarse category of a user-facing error, mapped from a bridge event kind or a
/// failing repository command. The detail text is [message].
enum class RepositoryErrorKind {
    CORE,
    CONNECTION_FAILED,
    NETWORK,
    PROTOCOL,
    PAIRING_FAILED,
    COMMAND,
}

/// A user-facing error surfaced through the repository.
///
/// [operation] localizes the failure to the thing that went wrong, so the UI can
/// say "Unable to load devices" rather than a generic failure (doc 14 §34), and
/// the message is a bridge-level message rather than a raw Rust error (doc 15
/// "Error handling"). Never carries clipboard content (doc 13 §44). The list is
/// bounded and newest first; [id] lets a ViewModel dismiss a single entry.
data class RepositoryError(
    val id: String,
    val kind: RepositoryErrorKind,
    val operation: String,
    val message: String,
    val deviceId: String?,
    val deviceName: String?,
    val severity: RepositoryErrorSeverity,
    val timestampMs: Long,
)
