package com.abram056.clipsync.core

import android.content.Context
import com.abram056.clipsync.data.ClipSyncRepository
import com.abram056.clipsync.data.CoreEventCollector
import com.abram056.clipsync.data.Device
import com.abram056.clipsync.data.HistoryItem
import com.abram056.clipsync.data.Identity
import com.abram056.clipsync.data.PairingRequest
import com.abram056.clipsync.data.RefreshKind
import com.abram056.clipsync.data.RepositoryError
import com.abram056.clipsync.data.RepositoryErrorKind
import com.abram056.clipsync.data.RepositoryErrorSeverity
import com.abram056.clipsync.data.SyncState
import com.abram056.clipsync.data.toDevice
import com.abram056.clipsync.data.toHistoryItem
import com.abram056.clipsync.data.toPairingRequest
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.withContext

/// Lifecycle state of the application-scoped [CoreManager].
sealed interface CoreManagerState {
    data object Stopped : CoreManagerState

    data object Starting : CoreManagerState

    data object Running : CoreManagerState

    data class Failed(val message: String) : CoreManagerState
}

private const val DEFAULT_DISCOVERY_PORT = 48271
private const val DEFAULT_LISTEN_PORT = 48272
private val HISTORY_LIMIT = 100u

/// Application-scoped owner of the single Rust core instance.
///
/// Guarantees exactly one active [ClipSyncCore] per Android process: activities,
/// view models, and services all reach the core through this manager instead of
/// creating their own (doc 15 §7, doc 13 §4–6). It creates and starts the core,
/// coordinates the [CoreEventCollector] and refresh pipeline, exposes the
/// repository-facing state and commands, and surfaces initialization failures
/// through [state].
///
/// It must not implement synchronization, own Compose state, implement
/// clipboard monitoring, or contain UI logic.
class CoreManager(
    context: Context,
    private val config: BridgeConfig = defaultConfig(context),
) : ClipSyncRepository {

    private val collector = CoreEventCollector()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val mutex = Mutex()

    @Volatile
    private var core: ClipSyncCore? = null
    private var refreshJob: Job? = null

    private val _state = MutableStateFlow<CoreManagerState>(CoreManagerState.Stopped)
    val state: StateFlow<CoreManagerState> = _state.asStateFlow()

    /// The running core, or null when stopped. Every consumer in the process
    /// observes the same instance while it is running.
    val activeCore: ClipSyncCore?
        get() = core

    override val identity: StateFlow<Identity?> = collector.identity
    override val devices: StateFlow<List<Device>> = collector.devices
    override val discoveredDevices: StateFlow<List<Device>> = collector.discoveredDevices
    override val pairingRequests: StateFlow<List<PairingRequest>> = collector.pairingRequests
    override val history: StateFlow<List<HistoryItem>> = collector.history
    override val recentClipboard: StateFlow<HistoryItem?> = collector.recentClipboard
    override val syncState: StateFlow<SyncState> = collector.syncState
    override val errors: StateFlow<List<RepositoryError>> = collector.errors

    /// Remote clipboard content to be written to the Android clipboard; the
    /// foreground service wires it to its ClipboardWriter (doc 13 §15/§23).
    val remoteClipboard: SharedFlow<String> = collector.remoteClipboard

    override suspend fun start() {
        ensureStarted()
    }

    /// Returns the running core, starting it first if needed. Idempotent:
    /// concurrent or repeated calls observe the same instance and never create
    /// a second core.
    suspend fun ensureStarted(): ClipSyncCore {
        mutex.lock()
        try {
            core?.let { return it }
            _state.value = CoreManagerState.Starting
            val started = try {
                withContext(Dispatchers.IO) { ClipSyncCore.start(config) }
            } catch (error: CancellationException) {
                _state.value = CoreManagerState.Stopped
                throw error
            } catch (error: Throwable) {
                _state.value =
                    CoreManagerState.Failed(error.message ?: "core failed to start")
                recordCoreFailure()
                throw error
            }
            core = started
            try {
                withContext(Dispatchers.IO) {
                    loadSnapshot(started)
                    started.subscribe(collector)
                }
            } catch (error: Throwable) {
                core = null
                runCatching { started.stop() }
                _state.value =
                    CoreManagerState.Failed(error.message ?: "core failed to initialize")
                if (error !is CancellationException) recordCoreFailure()
                throw error
            }
            refreshJob = scope.launch {
                collector.refreshRequests.collect { refresh(it) }
            }
            // A successful (re)start resolves any recorded startup failure.
            collector.clearErrors(RepositoryErrorKind.CORE)
            _state.value = CoreManagerState.Running
            return started
        } finally {
            mutex.unlock()
        }
    }

    override suspend fun stop() {
        mutex.lock()
        try {
            refreshJob?.cancel()
            refreshJob = null
            val current = core
            core = null
            if (current != null) {
                // NonCancellable so an orderly core shutdown still completes if
                // the awaiting caller is cancelled mid-stop.
                withContext(NonCancellable + Dispatchers.IO) {
                    runCatching { current.unsubscribe() }
                    current.stop()
                }
            }
            _state.value = CoreManagerState.Stopped
        } finally {
            mutex.unlock()
        }
    }

    override suspend fun requestPairing(deviceId: String) =
        call("Pairing request") { it.requestPairing(deviceId) }

    override suspend fun approvePairing(deviceId: String) =
        call("Approve pairing") { it.approvePairing(deviceId) }

    override suspend fun rejectPairing(deviceId: String, reason: String) =
        call("Reject pairing") { it.rejectPairing(deviceId, reason) }

    override suspend fun forgetDevice(deviceId: String) = call("Forget device") { it.forgetDevice(deviceId) }

    override suspend fun restoreHistory(id: String) = call("Restore history") { it.restoreHistoryEntry(id) }

    override suspend fun clearHistory() = call("Clear history") { it.clearHistory() }

    override suspend fun setPaused(paused: Boolean) = call("Synchronization") { it.setPaused(paused) }

    override suspend fun onClipboardChanged(content: String) = call("Clipboard capture") {
        it.onClipboardChanged(content)
    }

    override fun dismissError(id: String) = collector.dismissError(id)

    override fun clearErrors() = collector.clearErrors()

    /// Run a bridge command, recording a user-facing [RepositoryError] before
    /// rethrowing so a failed operation never fails silently (doc 15 §10,
    /// doc 14 §34). Cancellation is not a failure and is never recorded.
    private suspend fun call(operation: String, block: (ClipSyncCore) -> Unit) {
        val current = core ?: run {
            recordCommandFailure(operation, "Synchronization is not running")
            throw IllegalStateException("core manager not started")
        }
        try {
            withContext(Dispatchers.IO) { block(current) }
        } catch (error: CancellationException) {
            throw error
        } catch (error: Throwable) {
            recordCommandFailure(operation, error.message?.takeIf { it.isNotBlank() } ?: "Request failed")
            throw error
        }
    }

    private fun recordCommandFailure(operation: String, message: String) {
        collector.recordError(
            kind = RepositoryErrorKind.COMMAND,
            operation = operation,
            message = message,
            deviceId = null,
            deviceName = null,
            severity = RepositoryErrorSeverity.TRANSIENT,
            timestampMs = System.currentTimeMillis(),
        )
    }

    /// Record a core startup failure as a user-facing error. The message is
    /// deliberately generic: the raw Rust error stays in [CoreManagerState.Failed]
    /// for diagnostics rather than being surfaced as UI copy (doc 15 "Error
    /// handling"). Marked persistent until a later start succeeds and clears it.
    private fun recordCoreFailure() {
        collector.recordError(
            kind = RepositoryErrorKind.CORE,
            operation = "Core startup",
            message = "ClipSync failed to start",
            deviceId = null,
            deviceName = null,
            severity = RepositoryErrorSeverity.PERSISTENT,
            timestampMs = System.currentTimeMillis(),
        )
    }

    private fun loadSnapshot(core: ClipSyncCore) {
        val identity = core.identity()
        val trusted = core.trustedPeers().map { it.toDevice() }
        val discovered = core.discoveredPeers().map { it.toDevice() }
        val pairing = core.pendingPairingRequests().map { it.toPairingRequest() }
        val history = core.history(HISTORY_LIMIT).map { it.toHistoryItem() }
        val paused = core.isPaused()
        val size = core.historySizeBytes()
        collector.seedSnapshot(
            identity = Identity(deviceId = identity.deviceId, deviceName = identity.deviceName),
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

    companion object {
        /// Default production configuration: durable storage in the app's
        /// private files directory and the core's documented default ports.
        fun defaultConfig(context: Context): BridgeConfig = BridgeConfig(
            storagePath = context.applicationContext.filesDir.resolve("clipsync.db").absolutePath,
            discoveryPort = DEFAULT_DISCOVERY_PORT.toUShort(),
            listenPort = DEFAULT_LISTEN_PORT.toUShort(),
            discoveryTargets = emptyList(),
            syncEnabled = true,
        )
    }
}
