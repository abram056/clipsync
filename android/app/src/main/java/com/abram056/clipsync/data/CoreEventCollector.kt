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

/// Upper bound on retained user-facing errors (doc 15 §10).
private const val ERROR_LIMIT = 20

/// Translates bridge events into repository state.
///
/// Implements the generated `ClipSyncEventListener` so the bridge can invoke it
/// directly from the Rust event forwarder. All state is held in [StateFlow]s so
/// the repository can expose it to the UI. The collector maintains live
/// connection state from `DeviceConnected`/`DeviceDisconnected` events, per the
/// bridge specification.
class CoreEventCollector : ClipSyncEventListener {

    private val _identity = MutableStateFlow<Identity?>(null)
    val identity: StateFlow<Identity?> = _identity.asStateFlow()

    private val _devices = MutableStateFlow<List<Device>>(emptyList())
    val devices: StateFlow<List<Device>> = _devices.asStateFlow()

    private val _discoveredDevices = MutableStateFlow<List<Device>>(emptyList())
    val discoveredDevices: StateFlow<List<Device>> = _discoveredDevices.asStateFlow()

    private val _pairingRequests = MutableStateFlow<List<PairingRequest>>(emptyList())
    val pairingRequests: StateFlow<List<PairingRequest>> = _pairingRequests.asStateFlow()

    private val _history = MutableStateFlow<List<HistoryItem>>(emptyList())
    val history: StateFlow<List<HistoryItem>> = _history.asStateFlow()

    /// Newest history entry, exposed as first-class state for the Home screen
    /// preview (doc 15 §10, doc 14 §43). Written only through [publishHistory]
    /// so it can never drift from the history list, and written synchronously so
    /// a reader that just observed a new history list always sees the matching
    /// recent entry.
    private val _recentClipboard = MutableStateFlow<HistoryItem?>(null)
    val recentClipboard: StateFlow<HistoryItem?> = _recentClipboard.asStateFlow()

    private val _syncState = MutableStateFlow(SyncState())
    val syncState: StateFlow<SyncState> = _syncState.asStateFlow()

    /// User-facing errors, newest first, bounded by [ERROR_LIMIT] (doc 14 §35).
    private val _errors = MutableStateFlow<List<RepositoryError>>(emptyList())
    val errors: StateFlow<List<RepositoryError>> = _errors.asStateFlow()

    private val _refreshRequests = MutableSharedFlow<RefreshKind>(extraBufferCapacity = 4)
    val refreshRequests: SharedFlow<RefreshKind> = _refreshRequests.asSharedFlow()

    /// Remote clipboard content waiting to be applied to the Android clipboard
    /// by the foreground service's ClipboardWriter (doc 13 §15/§23).
    private val _remoteClipboard = MutableSharedFlow<String>(extraBufferCapacity = 8)
    val remoteClipboard: SharedFlow<String> = _remoteClipboard.asSharedFlow()

    /// Seed the collector from an authoritative snapshot loaded from the core.
    fun seedSnapshot(
        identity: Identity?,
        trusted: List<Device>,
        discovered: List<Device>,
        pairing: List<PairingRequest>,
        history: List<HistoryItem>,
        sync: SyncState,
    ) {
        _identity.value = identity
        _devices.value = trusted
        _discoveredDevices.value = discovered
        _pairingRequests.value = pairing
        publishHistory(history)
        _syncState.value = sync
    }

    /// Replace the history list after a targeted re-query.
    fun setHistory(history: List<HistoryItem>) {
        publishHistory(history)
    }

    /// The single write path for history, so [recentClipboard] is always derived
    /// from [history] in the same step.
    private fun publishHistory(history: List<HistoryItem>) {
        _history.value = history
        _recentClipboard.value = history.firstOrNull()
    }

    /// Record a user-facing error, keeping only the newest [ERROR_LIMIT].
    fun recordError(
        kind: RepositoryErrorKind,
        operation: String,
        message: String,
        deviceId: String?,
        deviceName: String?,
        severity: RepositoryErrorSeverity,
        timestampMs: Long,
    ) {
        _errors.update { current ->
            val error = RepositoryError(
                id = java.util.UUID.randomUUID().toString(),
                kind = kind,
                operation = operation,
                message = message,
                deviceId = deviceId,
                deviceName = deviceName,
                severity = severity,
                timestampMs = timestampMs,
            )
            (listOf(error) + current).take(ERROR_LIMIT)
        }
    }

    /// Dismiss a single error once the user acknowledges it.
    fun dismissError(id: String) {
        _errors.update { current -> current.filterNot { it.id == id } }
    }

    /// Clear every recorded error (e.g. when the failing condition recovers).
    fun clearErrors() {
        _errors.value = emptyList()
    }

    /// Clear the errors of a single kind, so a condition that has resolved (a
    /// successful core restart, a re-established connection) does not leave a
    /// persistent error on screen forever (doc 14 §35).
    fun clearErrors(kind: RepositoryErrorKind) {
        _errors.update { list -> list.filterNot { it.kind == kind } }
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

            BridgeEventKind.DEVICE_CONNECTION_FAILED ->
                addErrorFromEvent(event, RepositoryErrorKind.CONNECTION_FAILED, RepositoryErrorSeverity.PERSISTENT)

            BridgeEventKind.PAIRING_FAILED ->
                addErrorFromEvent(event, RepositoryErrorKind.PAIRING_FAILED, RepositoryErrorSeverity.TRANSIENT)

            BridgeEventKind.NETWORK_ERROR ->
                addErrorFromEvent(event, RepositoryErrorKind.NETWORK, RepositoryErrorSeverity.TRANSIENT)

            BridgeEventKind.PROTOCOL_ERROR ->
                addErrorFromEvent(event, RepositoryErrorKind.PROTOCOL, RepositoryErrorSeverity.PERSISTENT)

            BridgeEventKind.CLIPBOARD_UPDATED_FROM_REMOTE ->
                event.clipboard?.content?.let { content ->
                    if (content.isNotEmpty()) _remoteClipboard.tryEmit(content)
                }

            BridgeEventKind.CLIPBOARD_RESTORED ->
                _refreshRequests.tryEmit(RefreshKind.HISTORY)

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

    /// Error events carry their detail text in the device payload's `reason`
    /// field (doc 12 §29; bridge `with_device`), not a dedicated message field.
    private fun addErrorFromEvent(
        event: BridgeEvent,
        kind: RepositoryErrorKind,
        severity: RepositoryErrorSeverity,
    ) {
        val device = event.device
        recordError(
            kind = kind,
            operation = defaultOperation(kind),
            message = device?.reason?.takeIf { it.isNotBlank() } ?: defaultErrorMessage(kind),
            deviceId = device?.deviceId?.takeIf { it.isNotBlank() },
            deviceName = device?.deviceName?.takeIf { it.isNotBlank() },
            severity = severity,
            timestampMs = event.timestampMs,
        )
    }

    private fun defaultOperation(kind: RepositoryErrorKind): String = when (kind) {
        RepositoryErrorKind.CORE -> "Core startup"
        RepositoryErrorKind.CONNECTION_FAILED -> "Device connection"
        RepositoryErrorKind.NETWORK -> "Synchronization"
        RepositoryErrorKind.PROTOCOL -> "Device connection"
        RepositoryErrorKind.PAIRING_FAILED -> "Pairing"
        RepositoryErrorKind.COMMAND -> "Request"
    }

    private fun defaultErrorMessage(kind: RepositoryErrorKind): String = when (kind) {
        RepositoryErrorKind.CORE -> "Core failed to start"
        RepositoryErrorKind.CONNECTION_FAILED -> "Device connection failed"
        RepositoryErrorKind.NETWORK -> "Network error"
        RepositoryErrorKind.PROTOCOL -> "Protocol error"
        RepositoryErrorKind.PAIRING_FAILED -> "Pairing failed"
        RepositoryErrorKind.COMMAND -> "Request failed"
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
