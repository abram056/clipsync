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
