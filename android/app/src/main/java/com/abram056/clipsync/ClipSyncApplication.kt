package com.abram056.clipsync

import android.app.Application
import com.abram056.clipsync.core.CoreManager

class ClipSyncApplication : Application() {

    /// Single application-scoped owner of the Rust core. Outlives activities,
    /// so recreation never restarts the core (doc 13 §6).
    val coreManager: CoreManager by lazy { CoreManager(this) }
}
