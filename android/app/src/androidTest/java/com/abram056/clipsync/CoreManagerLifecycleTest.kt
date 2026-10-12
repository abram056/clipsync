package com.abram056.clipsync

import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.abram056.clipsync.core.BridgeConfig
import com.abram056.clipsync.core.CoreManager
import com.abram056.clipsync.core.CoreManagerState
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith

/// Milestone 4 gate (doc 15 §7): exactly one active Rust core per process.
///
/// Opening the activity, recreating it, and accessing it from multiple
/// consumers must all observe the same application-scoped [CoreManager] and the
/// same [com.abram056.clipsync.core.ClipSyncCore] instance.
@RunWith(AndroidJUnit4::class)
class CoreManagerLifecycleTest {

    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    private fun config(storageName: String) = BridgeConfig(
        storagePath = context.filesDir.resolve(storageName).absolutePath,
        discoveryPort = 58471.toUShort(),
        listenPort = 58472.toUShort(),
        discoveryTargets = emptyList(),
        syncEnabled = true,
    )

    @Test
    fun startIsIdempotentAndStopReleasesCore() = runBlocking {
        val manager = CoreManager(context, config("clipsync-m4-idempotent.db"))

        val first = manager.ensureStarted()
        val second = manager.ensureStarted()
        assertSame(first, second)
        assertSame(first, manager.activeCore)
        assertEquals(CoreManagerState.Running, manager.state.value)

        manager.stop()
        assertNull(manager.activeCore)
        assertEquals(CoreManagerState.Stopped, manager.state.value)
    }

    @Test
    fun activityRecreationKeepsSingleCore() = runBlocking {
        val application = ApplicationProvider.getApplicationContext<ClipSyncApplication>()
        val manager = application.coreManager

        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            awaitRunning(manager)
            val before = manager.activeCore
            assertNotNull(before)

            scenario.recreate()

            awaitRunning(manager)
            assertSame(before, manager.activeCore)
        }

        manager.stop()
    }

    @Test
    fun multipleViewModelsAndServiceShareSameCore() = runBlocking {
        val application = ApplicationProvider.getApplicationContext<ClipSyncApplication>()
        val manager = application.coreManager

        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            awaitRunning(manager)

            scenario.onActivity { activity ->
                val uiViewModel = ViewModelProvider(activity)[ClipSyncViewModel::class.java]
                val otherViewModel = ClipSyncViewModel(application)
                // The future SyncForegroundService resolves the same manager
                // from the application (doc 13 §9).
                val serviceManager = application.coreManager

                assertNotSame(uiViewModel, otherViewModel)
                assertSame(manager, uiViewModel.coreManager)
                assertSame(manager, otherViewModel.coreManager)
                assertSame(manager, serviceManager)
                assertSame(manager.activeCore, uiViewModel.coreManager.activeCore)
                assertSame(manager.activeCore, otherViewModel.coreManager.activeCore)
            }
        }

        manager.stop()
    }

    private suspend fun awaitRunning(manager: CoreManager) {
        withTimeout(15_000) {
            manager.state.first { it is CoreManagerState.Running }
        }
    }
}
