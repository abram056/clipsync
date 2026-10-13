package com.abram056.clipsync.data

import com.abram056.clipsync.core.BridgeEvent
import com.abram056.clipsync.core.BridgeEventKind
import com.abram056.clipsync.core.ClipSyncEventListener
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/// Signals that the repository must re-query the core to refresh state.
enum class RefreshKind {
    HISTORY,
    ALL,
}

/// Translates bridge events into repository state.
///
/// Implements the generated `ClipSyncEventListener` so the bridge can invoke it
/// directly from the Rust event forwarder. All state is held in [StateFlow]s so
/// the repository can expose it to the UI. The collector maintains live
/// connection state from `DeviceConnected`/`DeviceDisconnected` events, per the
/// bridge specification.
class CoreEventCollector : ClipSyncEventListener {

    private val _devices = MutableStateFlow<List<Device>>(emptyList())
    val devices: StateFlow<List<Device>> = _devices.asStateFlow()

    private val _discoveredDevices = MutableStateFlow<List<Device>>(emptyList())
    val discoveredDevices: StateFlow<List<Device>> = _discoveredDevices.asStateFlow()

    private val _pairingRequests = MutableStateFlow<List<PairingRequest>>(emptyList())
    val pairingRequests: StateFlow<List<PairingRequest>> = _pairingRequests.asStateFlow()

    private val _history = MutableStateFlow<List<HistoryItem>>(emptyList())
    val history: StateFlow<List<HistoryItem>> = _history.asStateFlow()

    private val _syncState = MutableStateFlow(SyncState())
    val syncState: StateFlow<SyncState> = _syncState.asStateFlow()

    private val _refreshRequests = MutableSharedFlow<RefreshKind>(extraBufferCapacity = 4)
    val refreshRequests: SharedFlow<RefreshKind> = _refreshRequests.asSharedFlow()

    /// Remote clipboard content waiting to be applied to the Android clipboard
    /// by the foreground service's ClipboardWriter (doc 13 §15/§23).
    private val _remoteClipboard = MutableSharedFlow<String>(extraBufferCapacity = 8)
    val remoteClipboard: SharedFlow<String> = _remoteClipboard.asSharedFlow()

    /// Seed the collector from an authoritative snapshot loaded from the core.
    fun seedSnapshot(
        trusted: List<Device>,
        discovered: List<Device>,
        pairing: List<PairingRequest>,
        history: List<HistoryItem>,
        sync: SyncState,
    ) {
        _devices.value = trusted
        _discoveredDevices.value = discovered
        _pairingRequests.value = pairing
        _history.value = history
        _syncState.value = sync
    }

    /// Replace the history list after a targeted re-query.
    fun setHistory(history: List<HistoryItem>) {
        _history.value = history
    }

    override fun onEvent(event: BridgeEvent) {
        when (event.kind) {
            BridgeEventKind.DEVICE_CONNECTED ->
                event.device?.let { setConnected(it.deviceId, true) }

            BridgeEventKind.DEVICE_DISCONNECTED ->
                event.device?.let { setConnected(it.deviceId, false) }

            BridgeEventKind.DEVICE_DISCOVERED ->
                event.device?.let { addDiscovered(it.deviceId, it.deviceName, it.platform) }

            BridgeEventKind.PAIRING_REQUESTED ->
                event.device?.let { addPairingRequest(it.deviceId, it.deviceName, it.platform, event.timestampMs) }

            BridgeEventKind.PAIRING_ACCEPTED ->
                event.device?.let { removePairingRequest(it.deviceId) }

            BridgeEventKind.PAIRING_REJECTED ->
                event.device?.let { removePairingRequest(it.deviceId) }

            BridgeEventKind.CLIPBOARD_UPDATED_FROM_REMOTE ->
                event.clipboard?.content?.let { content ->
                    if (content.isNotEmpty()) _remoteClipboard.tryEmit(content)
                }

            BridgeEventKind.CLIPBOARD_HISTORY_UPDATED -> {
                event.sync?.let { sync ->
                    _syncState.update { it.copy(historyBytes = sync.historyBytes) }
                }
                _refreshRequests.tryEmit(RefreshKind.HISTORY)
            }

            BridgeEventKind.SYNC_COMPLETED ->
                event.sync?.let { sync ->
                    _syncState.update { it.copy(peerCount = sync.peerCount.toInt()) }
                }

            BridgeEventKind.EVENT_STREAM_LAGGED ->
                _refreshRequests.tryEmit(RefreshKind.ALL)

            else -> Unit
        }
    }

    private fun setConnected(deviceId: String, connected: Boolean) {
        _devices.update { list ->
            list.map { device ->
                if (device.deviceId == deviceId) device.copy(isConnected = connected) else device
            }
        }
    }

    private fun addDiscovered(deviceId: String, deviceName: String, platform: String) {
        _discoveredDevices.update { list ->
            if (list.any { it.deviceId == deviceId }) {
                list
            } else {
                list + Device(
                    deviceId = deviceId,
                    deviceName = deviceName,
                    platform = platform,
                    isTrusted = false,
                    isConnected = false,
                    lastSeenMs = null,
                )
            }
        }
    }

    private fun addPairingRequest(deviceId: String, deviceName: String, platform: String, receivedAtMs: Long) {
        _pairingRequests.update { list ->
            if (list.any { it.deviceId == deviceId }) {
                list
            } else {
                list + PairingRequest(
                    requestId = java.util.UUID.randomUUID().toString(),
                    deviceId = deviceId,
                    deviceName = deviceName,
                    platform = platform,
                    receivedAtMs = receivedAtMs,
                )
            }
        }
    }

    private fun removePairingRequest(deviceId: String) {
        _pairingRequests.update { list -> list.filterNot { it.deviceId == deviceId } }
    }
}
