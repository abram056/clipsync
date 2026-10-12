package com.abram056.clipsync

import android.app.NotificationManager
import android.content.Intent
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.abram056.clipsync.core.CoreManagerState
import com.abram056.clipsync.service.SyncForegroundService
import com.abram056.clipsync.service.SyncNotification
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith

/// Milestone 5 gate (doc 15 §8): the foreground service keeps the core alive
/// when the UI is not visible and shows a persistent notification.
@RunWith(AndroidJUnit4::class)
class SyncForegroundServiceTest {

    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val serviceIntent by lazy {
        Intent(context, SyncForegroundService::class.java)
    }

    @Test
    fun serviceKeepsCoreAliveAndShowsNotification() = runBlocking {
        val application = context.applicationContext as ClipSyncApplication
        val manager = application.coreManager

        // Clean slate regardless of prior test-class state.
        runCatching { manager.stop() }

        context.startForegroundService(serviceIntent)

        withTimeout(15_000) {
            manager.state.first { it is CoreManagerState.Running }
        }
        val core = manager.activeCore
        assertNotNull(core)

        // The persistent notification is posted.
        val notificationManager = context.getSystemService(NotificationManager::class.java)
        withTimeout(10_000) {
            while (notificationManager.activeNotifications.none { it.id == SyncNotification.NOTIF_ID }) {
                delay(200)
            }
        }

        // A second start keeps the same core (idempotent, no duplicate).
        context.startForegroundService(serviceIntent)
        delay(1_000)
        assertSame(core, manager.activeCore)

        // Stopping the service leaves the core running: closing the service is
        // not an explicit sync disable (doc 13 §7).
        context.stopService(serviceIntent)
        delay(1_000)
        assertSame(core, manager.activeCore)

        manager.stop()
    }
}
