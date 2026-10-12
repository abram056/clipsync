package com.abram056.clipsync

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.abram056.clipsync.core.CoreManager
import com.abram056.clipsync.core.CoreManagerState
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/// Seeds the ViewModel pattern that the M10 UI will build on.
///
/// Talks only to the application-scoped [CoreManager]; requesting a start is
/// idempotent, so opening the activity (or recreating it) never creates a
/// second core. Its lifecycle cannot stop synchronization — closing the UI
/// leaves the core running (doc 15 §7).
class ClipSyncViewModel(application: Application) : AndroidViewModel(application) {

    internal val coreManager: CoreManager = (application as ClipSyncApplication).coreManager

    val coreState: StateFlow<CoreManagerState> = coreManager.state

    fun ensureStarted() {
        viewModelScope.launch {
            runCatching { coreManager.ensureStarted() }
        }
    }
}
