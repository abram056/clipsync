package com.abram056.clipsync

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.abram056.clipsync.core.BridgeConfig
import com.abram056.clipsync.core.CoreManager
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class RepositoryEventPipelineTest {

    private fun config(storagePath: String) = BridgeConfig(
        storagePath = storagePath,
        discoveryPort = 58371.toUShort(),
        listenPort = 58372.toUShort(),
        discoveryTargets = emptyList(),
        syncEnabled = true,
    )

    @Test
    fun m3EventPipelineUpdatesRepositoryState() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        // Unique per run so persisted history never leaks into the exact-count
        // assertions below (the app's storage survives across suite runs).
        val storage = context.filesDir
            .resolve("clipsync-m3-gate-${System.nanoTime()}.db")
            .absolutePath
        val coreManager = CoreManager(context, config(storage))

        coreManager.start()

        coreManager.onClipboardChanged("m3 gate content")

        val updatedHistory = withTimeout(10_000) {
            coreManager.history.first { it.isNotEmpty() }
        }
        assertEquals(1, updatedHistory.size)
        assertEquals("m3 gate content", updatedHistory[0].content)

        // M7: the repository exposes the extra doc 15 §10 surfaces alongside the
        // pipeline, so the UI can read snapshot + event state without touching Rust.
        assertEquals(updatedHistory[0], coreManager.recentClipboard.value)

        // The predicate guarantees non-null; the assertion keeps the type non-null.
        val identity = withTimeout(10_000) { coreManager.identity.first { it != null } }!!
        assertTrue(identity.deviceId.isNotBlank())
        assertTrue(identity.deviceName.isNotBlank())

        coreManager.stop()
    }
}
