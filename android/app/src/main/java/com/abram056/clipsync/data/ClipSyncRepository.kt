package com.abram056.clipsync.data

import com.abram056.clipsync.core.BridgeDevice
import com.abram056.clipsync.core.BridgeHistoryEntry
import com.abram056.clipsync.core.BridgePairingRequest
import kotlinx.coroutines.flow.StateFlow

internal fun BridgeDevice.toDevice() = Device(
    deviceId = deviceId,
    deviceName = deviceName,
    platform = platform,
    isTrusted = isTrusted,
    isConnected = isConnected,
    lastSeenMs = lastSeenMs,
)

internal fun BridgePairingRequest.toPairingRequest() = PairingRequest(
    requestId = requestId,
    deviceId = deviceId,
    deviceName = deviceName,
    platform = platform,
    receivedAtMs = receivedAtMs,
)

internal fun BridgeHistoryEntry.toHistoryItem() = HistoryItem(
    clipboardId = clipboardId,
    content = content,
    contentType = contentType,
    contentHash = contentHash,
    originDeviceId = originDeviceId,
    createdAtMs = createdAtMs,
    sizeBytes = sizeBytes,
)

/// Boundary between generated UniFFI bindings and application code (doc 12 §28).
///
/// The application-scoped [com.abram056.clipsync.core.CoreManager] implements
/// this interface; view models and services talk to it instead of the generated
/// API directly.
interface ClipSyncRepository {
    val devices: StateFlow<List<Device>>
    val discoveredDevices: StateFlow<List<Device>>
    val pairingRequests: StateFlow<List<PairingRequest>>
    val history: StateFlow<List<HistoryItem>>
    val syncState: StateFlow<SyncState>

    suspend fun start()
    suspend fun stop()

    suspend fun requestPairing(deviceId: String)
    suspend fun approvePairing(deviceId: String)
    suspend fun rejectPairing(deviceId: String, reason: String)
    suspend fun forgetDevice(deviceId: String)

    suspend fun restoreHistory(id: String)
    suspend fun clearHistory()
    suspend fun setPaused(paused: Boolean)
    suspend fun onClipboardChanged(content: String)
}
