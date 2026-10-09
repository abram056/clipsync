package com.abram056.clipsync.data

import com.abram056.clipsync.core.BridgeConfig
import com.abram056.clipsync.core.BridgeDevice
import com.abram056.clipsync.core.BridgeHistoryEntry
import com.abram056.clipsync.core.BridgePairingRequest
import com.abram056.clipsync.core.ClipSyncCore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private val HISTORY_LIMIT = 100u

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

interface ClipSyncRepository {
    val devices: StateFlow<List<Device>>
    val discoveredDevices: StateFlow<List<Device>>
    val pairingRequests: StateFlow<List<PairingRequest>>
    val history: StateFlow<List<HistoryItem>>
    val syncState: StateFlow<SyncState>

    suspend fun start(config: BridgeConfig)
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

class ClipSyncRepositoryImpl : ClipSyncRepository {

    private val collector = CoreEventCollector()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var core: ClipSyncCore? = null
    private var refreshJob: Job? = null

    override val devices: StateFlow<List<Device>> = collector.devices
    override val discoveredDevices: StateFlow<List<Device>> = collector.discoveredDevices
    override val pairingRequests: StateFlow<List<PairingRequest>> = collector.pairingRequests
    override val history: StateFlow<List<HistoryItem>> = collector.history
    override val syncState: StateFlow<SyncState> = collector.syncState

    override suspend fun start(config: BridgeConfig) = withContext(Dispatchers.IO) {
        check(core == null) { "repository already started" }
        val started = ClipSyncCore.start(config)
        core = started
        loadSnapshot(started)
        started.subscribe(collector)
        refreshJob = scope.launch {
            collector.refreshRequests.collect { kind -> refresh(kind) }
        }
    }

    override suspend fun stop() {
        withContext(Dispatchers.IO) {
            refreshJob?.cancel()
            refreshJob = null
            val current = core ?: return@withContext
            core = null
            current.unsubscribe()
            current.stop()
        }
    }

    override suspend fun requestPairing(deviceId: String) = call { it.requestPairing(deviceId) }

    override suspend fun approvePairing(deviceId: String) = call { it.approvePairing(deviceId) }

    override suspend fun rejectPairing(deviceId: String, reason: String) =
        call { it.rejectPairing(deviceId, reason) }

    override suspend fun forgetDevice(deviceId: String) = call { it.forgetDevice(deviceId) }

    override suspend fun restoreHistory(id: String) = call { it.restoreHistoryEntry(id) }

    override suspend fun clearHistory() = call { it.clearHistory() }

    override suspend fun setPaused(paused: Boolean) = call { it.setPaused(paused) }

    override suspend fun onClipboardChanged(content: String) = call {
        it.onClipboardChanged(content)
    }

    private suspend fun call(block: (ClipSyncCore) -> Unit) = withContext(Dispatchers.IO) {
        val current = core ?: error("repository not started")
        block(current)
    }

    private fun loadSnapshot(core: ClipSyncCore) {
        val trusted = core.trustedPeers().map { it.toDevice() }
        val discovered = core.discoveredPeers().map { it.toDevice() }
        val pairing = core.pendingPairingRequests().map { it.toPairingRequest() }
        val history = core.history(HISTORY_LIMIT).map { it.toHistoryItem() }
        val paused = core.isPaused()
        val size = core.historySizeBytes()
        collector.seedSnapshot(
            trusted = trusted,
            discovered = discovered,
            pairing = pairing,
            history = history,
            sync = SyncState(isPaused = paused, historyBytes = size),
        )
    }

    private fun refresh(kind: RefreshKind) {
        val current = core ?: return
        when (kind) {
            RefreshKind.HISTORY -> {
                collector.setHistory(current.history(HISTORY_LIMIT).map { it.toHistoryItem() })
            }
            RefreshKind.ALL -> loadSnapshot(current)
        }
    }
}
