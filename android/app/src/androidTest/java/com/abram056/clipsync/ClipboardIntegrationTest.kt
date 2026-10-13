package com.abram056.clipsync

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.abram056.clipsync.core.BridgeConfig
import com.abram056.clipsync.core.BridgeEvent
import com.abram056.clipsync.core.BridgeEventKind
import com.abram056.clipsync.core.ClipboardEventPayload
import com.abram056.clipsync.core.CoreManager
import com.abram056.clipsync.core.CoreManagerState
import com.abram056.clipsync.data.CoreEventCollector
import com.abram056.clipsync.service.ClipboardFilter
import com.abram056.clipsync.service.ClipboardMonitor
import com.abram056.clipsync.service.ClipboardWriter
import com.abram056.clipsync.service.SyncForegroundService
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/// Milestone 6 gate (doc 15 §9): the Android clipboard connects to the Rust
/// core in both directions through the ClipboardMonitor / ClipboardWriter
/// adapters (doc 13 §11–16). The accessibility-capture half of the hybrid
/// mechanism cannot run on this API 26 device, so its contract is covered here
/// by the shared ClipboardFilter test double (doc 10 §4). The monitor's
/// OEM-fallback poll (this EMUI 8 device never dispatches the platform
/// clip-changed listener) is what carries the capture tests below.
@RunWith(AndroidJUnit4::class)
class ClipboardIntegrationTest {

    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val clipboard =
        context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

    private fun config(storageName: String) = BridgeConfig(
        storagePath = context.filesDir.resolve(storageName).absolutePath,
        discoveryPort = 58671.toUShort(),
        listenPort = 58672.toUShort(),
        discoveryTargets = emptyList(),
        syncEnabled = true,
    )

    private fun uniqueContent(prefix: String) = "$prefix-${System.nanoTime()}"

    private suspend fun awaitClipboardText(expected: String) = withTimeout(10_000) {
        while (clipboardText() != expected) delay(100)
    }

    private fun clipboardText(): String? {
        val clip = clipboard.primaryClip ?: return null
        return if (clip.itemCount > 0) clip.getItemAt(0).text?.toString() else null
    }

    @Test
    fun writerAppliesContentToSystemClipboard() = runBlocking {
        val expected = uniqueContent("writer-remote")
        ClipboardWriter(context).write(expected)
        awaitClipboardText(expected)
    }

    @Test
    fun remoteEventIsForwardedToClipboardWriter() = runBlocking {
        val expected = uniqueContent("collector-remote")
        val collector = CoreEventCollector()
        val writer = ClipboardWriter(context)

        val applied = launch {
            collector.remoteClipboard.collect { content -> writer.write(content) }
        }
        // Subscription must precede the emission (events in production only
        // flow once the foreground service has already subscribed).
        delay(150)

        collector.onEvent(
            BridgeEvent(
                eventId = "m6-remote-1",
                timestampMs = System.currentTimeMillis(),
                kind = BridgeEventKind.CLIPBOARD_UPDATED_FROM_REMOTE,
                device = null,
                clipboard = ClipboardEventPayload(
                    clipboardId = "m6-clip-1",
                    content = expected,
                    contentType = "text/plain",
                    contentHash = "hash",
                    originDeviceId = "peer",
                    createdAtMs = System.currentTimeMillis(),
                ),
                sync = null,
                droppedEvents = 0,
            ),
        )

        awaitClipboardText(expected)
        applied.cancel()
    }

    @Test
    fun filterCoalescesDuplicatesAndOversizedContent() = runBlocking {
        val forwarded = mutableListOf<String>()
        val filter = ClipboardFilter(
            onForward = { content -> forwarded += content },
            maxBytes = 64,
            debounceMs = 50,
        )
        filter.start()
        try {
            // Two rapid identical submissions coalesce into one forward.
            filter.submit("same")
            filter.submit("same")
            withTimeout(5_000) { while (forwarded.isEmpty()) delay(20) }

            // Re-submitting the already-forwarded content is dropped.
            filter.submit("same")
            // Empty and oversized submissions never reach the core.
            filter.submit("")
            filter.submit("a".repeat(65))
            // New content still forwards.
            filter.submit("other")
            withTimeout(5_000) {
                while (forwarded.size < 2) delay(20)
            }

            assertEquals(listOf("same", "other"), forwarded)
        } finally {
            filter.stop()
        }

        // Submissions while stopped are dropped.
        filter.submit("after-stop")
        delay(200)
        assertEquals(listOf("same", "other"), forwarded)
    }

    @Test
    fun localClipIsCapturedIntoCoreHistory() = runBlocking<Unit> {
        val manager = CoreManager(context, config("clipsync-m6-monitor.db"))
        val monitor = ClipboardMonitor.create(context, manager)
        val expected = uniqueContent("captured")
        try {
            manager.ensureStarted()
            monitor.start()

            clipboard.setPrimaryClip(ClipData.newPlainText(null, expected))

            withTimeout(15_000) {
                manager.history.first { list -> list.any { it.content == expected } }
            }
        } finally {
            monitor.stop()
            manager.stop()
        }
    }

    @Test
    fun foregroundServiceStartsMonitorOnRunning() = runBlocking<Unit> {
        val application = context.applicationContext as ClipSyncApplication
        val manager = application.coreManager
        val serviceIntent = Intent(context, SyncForegroundService::class.java)

        // Clean slate regardless of prior test-class state.
        context.stopService(serviceIntent)
        runCatching { manager.stop() }

        try {
            context.startForegroundService(serviceIntent)
            withTimeout(15_000) { manager.state.first { it is CoreManagerState.Running } }

            // The service attaches the monitor asynchronously after the core is
            // Running (doc 13 §33), and the poll baselines whatever is on the
            // clipboard at attach time. A copy issued in that window can land
            // before the baseline and be skipped as pre-existing, so retry with
            // fresh content: the first attempt guarantees the monitor is
            // attached, and a later attempt always round-trips into history.
            withTimeout(20_000) {
                var attempt = 0
                while (true) {
                    attempt += 1
                    val expected = uniqueContent("service-captured-$attempt")
                    clipboard.setPrimaryClip(ClipData.newPlainText(null, expected))
                    val captured = runCatching {
                        withTimeout(3_000) {
                            manager.history.first { list -> list.any { it.content == expected } }
                        }
                    }.isSuccess
                    if (captured) return@withTimeout
                    delay(500)
                }
            }
        } finally {
            context.stopService(serviceIntent)
            delay(500)
            runCatching { manager.stop() }
        }
    }
}
