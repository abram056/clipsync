package com.abram056.clipsync

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.abram056.clipsync.core.BridgeConfig
import com.abram056.clipsync.core.ClipSyncCore
import com.abram056.clipsync.core.ClipSyncException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class BridgeLifecycleTest {

    private fun config(storagePath: String) = BridgeConfig(
        storagePath = storagePath,
        discoveryPort = 58271.toUShort(),
        listenPort = 58272.toUShort(),
        discoveryTargets = emptyList(),
        syncEnabled = true,
    )

    @Test
    fun startRetrieveIdentityAndStop() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val storage = context.filesDir.resolve("clipsync-test.db").absolutePath
        val core = withContext(Dispatchers.IO) {
            ClipSyncCore.start(config(storage))
        }
        val identity = core.identity()
        assertTrue(identity.deviceId.isNotBlank())
        assertTrue(identity.deviceName.isNotBlank())
        core.stop()
    }

    @Test
    fun invalidConfigurationProducesUsefulError() {
        val error = assertThrows(ClipSyncException.InvalidArgument::class.java) {
            ClipSyncCore.start(config("   "))
        }
        assertTrue(error.message?.isNotBlank() == true)
    }
}
