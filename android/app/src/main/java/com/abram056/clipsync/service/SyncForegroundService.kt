package com.abram056.clipsync.service

import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import com.abram056.clipsync.ClipSyncApplication
import com.abram056.clipsync.R
import com.abram056.clipsync.core.CoreManager
import com.abram056.clipsync.core.CoreManagerState
import com.abram056.clipsync.data.SyncState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

/// Keeps synchronization alive when the UI is not visible (doc 15 §8).
///
/// The service only controls lifecycle: on start it asks the application-scoped
/// [CoreManager] for the (idempotent) core, keeps the process alive via a
/// persistent foreground notification, and tears itself down if the core is
/// explicitly stopped or unrecoverably failed (bounded retry — never an
/// infinite restart loop). It must not contain networking, pairing, sync
/// algorithms, or database logic.
class SyncForegroundService : Service() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var coreManager: CoreManager? = null
    private var retryJob: Job? = null
    private var retryAttempts = 0
    private var lastObservedState: CoreManagerState? = null

    override fun onCreate() {
        super.onCreate()
        SyncNotification.ensureChannel(this)
        val manager = (application as ClipSyncApplication).coreManager
        coreManager = manager
        // Observes once per service instance (onCreate runs exactly once).
        observeState(manager)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val manager = requireNotNull(coreManager)
        // startForeground must be called promptly; the text already reflects the
        // current state so a sticky restart shows accurate information.
        val text = textFor(manager.state.value, manager.syncState.value)
        val notification = SyncNotification.build(this, text)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                SyncNotification.NOTIF_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
            )
        } else {
            @Suppress("DEPRECATION")
            startForeground(SyncNotification.NOTIF_ID, notification)
        }
        // Idempotent: a repeated start never creates a second core (doc 13 §9).
        scope.launch { runCatching { manager.ensureStarted() } }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun observeState(manager: CoreManager) {
        scope.launch {
            combine(manager.state, manager.syncState) { state, sync -> state to sync }
                .collectLatest { (state, sync) -> handle(manager, state, sync) }
        }
    }

    private suspend fun handle(
        manager: CoreManager,
        state: CoreManagerState,
        sync: SyncState,
    ) {
        val previous = lastObservedState
        lastObservedState = state
        when (state) {
            is CoreManagerState.Stopped -> {
                // Ignore the initial Stopped emission before ensureStarted runs;
                // only tear down when the running core was explicitly stopped —
                // stopping the service itself is not an explicit sync disable.
                if (previous != null) stopSelf()
            }

            is CoreManagerState.Starting -> SyncNotification.update(this, textFor(state, sync))

            is CoreManagerState.Running -> {
                retryAttempts = 0
                SyncNotification.update(this, textFor(state, sync))
            }

            is CoreManagerState.Failed -> {
                SyncNotification.update(this, textFor(state, sync))
                if (retryAttempts < MAX_RETRIES) {
                    retryAttempts += 1
                    retryJob?.cancel()
                    retryJob = scope.launch {
                        delay(RETRY_DELAYS_MS[retryAttempts - 1])
                        runCatching { manager.ensureStarted() }
                    }
                } else {
                    stopSelf()
                }
            }
        }
    }

    private fun textFor(state: CoreManagerState, sync: SyncState): String = when (state) {
        is CoreManagerState.Starting -> getString(R.string.notification_starting)
        is CoreManagerState.Running ->
            if (sync.isPaused) {
                getString(R.string.notification_paused)
            } else {
                getString(R.string.notification_syncing, sync.peerCount)
            }

        is CoreManagerState.Failed -> getString(R.string.notification_unavailable)
        is CoreManagerState.Stopped -> getString(R.string.notification_starting)
    }

    override fun onDestroy() {
        retryJob?.cancel()
        coreManager = null
        scope.cancel()
        @Suppress("DEPRECATION")
        stopForeground(STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    companion object {
        private const val MAX_RETRIES = 2
        private val RETRY_DELAYS_MS = listOf(10_000L, 30_000L)
    }
}
