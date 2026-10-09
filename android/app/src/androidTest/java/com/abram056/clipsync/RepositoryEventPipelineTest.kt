package com.abram056.clipsync

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.abram056.clipsync.core.BridgeConfig
import com.abram056.clipsync.data.ClipSyncRepositoryImpl
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
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
        val storage = context.filesDir.resolve("clipsync-m3-gate.db").absolutePath
        val repository = ClipSyncRepositoryImpl()

        withContext(Dispatchers.IO) {
            repository.start(config(storage))
        }

        repository.onClipboardChanged("m3 gate content")

        val updatedHistory = withTimeout(10_000) {
            repository.history.first { it.isNotEmpty() }
        }
        assertEquals(1, updatedHistory.size)
        assertEquals("m3 gate content", updatedHistory[0].content)

        repository.stop()
    }
}
