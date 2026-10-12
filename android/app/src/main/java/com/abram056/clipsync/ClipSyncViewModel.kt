package com.abram056.clipsync

import android.app.Application
import android.content.Intent
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.abram056.clipsync.core.CoreManager
import com.abram056.clipsync.core.CoreManagerState
import com.abram056.clipsync.service.SyncForegroundService
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/// Seeds the ViewModel pattern that the M10 UI will build on.
///
/// Talks only to the application-scoped [CoreManager]; requesting a start is
/// idempotent, so opening the activity (or recreating it) never creates a
/// second core. Its lifecycle cannot stop synchronization — closing the UI
/// leaves the core running (doc 15 §7, Milestone 5).
class ClipSyncViewModel(application: Application) : AndroidViewModel(application) {

    internal val coreManager: CoreManager = (application as ClipSyncApplication).coreManager

    val coreState: StateFlow<CoreManagerState> = coreManager.state

    /// Requests that synchronization is running. Starting the foreground
    /// service also ensures the core (the service calls [CoreManager.ensureStarted]
    /// idempotently on start), so this is the single entry point for enabling
    /// synchronization and keeping it alive when the UI is not visible.
    fun ensureStarted() {
        viewModelScope.launch {
            runCatching { coreManager.ensureStarted() }
        }
        val app = getApplication<Application>()
        app.startForegroundService(Intent(app, SyncForegroundService::class.java))
    }
}
