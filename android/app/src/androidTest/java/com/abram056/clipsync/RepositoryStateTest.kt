package com.abram056.clipsync

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.abram056.clipsync.core.BridgeConfig
import com.abram056.clipsync.core.BridgeEvent
import com.abram056.clipsync.core.BridgeEventKind
import com.abram056.clipsync.core.CoreManager
import com.abram056.clipsync.core.DeviceEventPayload
import com.abram056.clipsync.data.CoreEventCollector
import com.abram056.clipsync.data.HistoryItem
import com.abram056.clipsync.data.RepositoryErrorKind
import com.abram056.clipsync.data.RepositoryErrorSeverity
import com.abram056.clipsync.data.SyncState
import java.util.UUID
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/// Milestone 7 gate (doc 15 §10): the Kotlin state layer is complete enough that
/// a consumer can read realistic repository state without ever touching Rust.
///
/// Covers the three surfaces added here beyond the doc 12 §28 core interface —
/// `identity` (doc 12 §30 initial snapshot), the derived `recentClipboard`
/// (doc 14 §43) and `errors` (doc 14 §35) — plus the bounded/dismissable error
/// list. Error-event mapping is exercised against synthetic bridge events, the
/// same construction the M6 tests already use.
@RunWith(AndroidJUnit4::class)
class RepositoryStateTest {

    private fun config(storagePath: String) = BridgeConfig(
        storagePath = storagePath,
        discoveryPort = 58771.toUShort(),
        listenPort = 58772.toUShort(),
        discoveryTargets = emptyList(),
        syncEnabled = true,
    )

    private fun bridgeEvent(kind: BridgeEventKind, device: DeviceEventPayload? = null) = BridgeEvent(
        eventId = UUID.randomUUID().toString(),
        timestampMs = System.currentTimeMillis(),
        kind = kind,
        device = device,
        clipboard = null,
        sync = null,
        droppedEvents = 0,
    )

    private fun historyItem(content: String, id: String = "clip-$content") = HistoryItem(
        clipboardId = id,
        content = content,
        contentType = "text/plain",
        contentHash = "hash-$content",
        originDeviceId = "local",
        createdAtMs = System.currentTimeMillis(),
        sizeBytes = content.length.toLong(),
    )

    @Test
    fun networkErrorEventIsMappedToRepositoryError() = runBlocking {
        val collector = CoreEventCollector()

        collector.onEvent(
            bridgeEvent(
                kind = BridgeEventKind.NETWORK_ERROR,
                device = DeviceEventPayload(
                    deviceId = "device-1",
                    deviceName = "Laptop",
                    platform = "linux",
                    reason = "socket timeout",
                ),
            ),
        )

        val error = collector.errors.value.single()
        assertEquals(RepositoryErrorKind.NETWORK, error.kind)
        assertEquals("Synchronization", error.operation)
        assertEquals("socket timeout", error.message)
        assertEquals("device-1", error.deviceId)
        assertEquals("Laptop", error.deviceName)
        assertEquals(RepositoryErrorSeverity.TRANSIENT, error.severity)
    }

    @Test
    fun connectionFailureIsReportedAsPersistent() = runBlocking {
        val collector = CoreEventCollector()

        collector.onEvent(
            bridgeEvent(
                kind = BridgeEventKind.DEVICE_CONNECTION_FAILED,
                device = DeviceEventPayload(
                    deviceId = "device-1",
                    deviceName = "Laptop",
                    platform = "linux",
                    reason = "connection refused",
                ),
            ),
        )

        val error = collector.errors.value.single()
        assertEquals(RepositoryErrorKind.CONNECTION_FAILED, error.kind)
        assertEquals("Device connection", error.operation)
        assertEquals(RepositoryErrorSeverity.PERSISTENT, error.severity)
    }

    @Test
    fun blankErrorPayloadFallsBackToGenericCopy() = runBlocking {
        val collector = CoreEventCollector()

        collector.onEvent(
            bridgeEvent(
                kind = BridgeEventKind.PROTOCOL_ERROR,
                device = DeviceEventPayload(
                    deviceId = "",
                    deviceName = "",
                    platform = "",
                    reason = "",
                ),
            ),
        )

        val error = collector.errors.value.single()
        assertEquals("Protocol error", error.message)
        assertEquals(RepositoryErrorKind.PROTOCOL, error.kind)
        assertEquals(RepositoryErrorSeverity.PERSISTENT, error.severity)
        // Blank identifiers are dropped rather than shown as empty strings.
        assertNull(error.deviceId)
        assertNull(error.deviceName)
    }

    @Test
    fun errorsAreBoundedDismissableAndClearable() = runBlocking {
        val collector = CoreEventCollector()

        repeat(25) { index ->
            collector.recordError(
                kind = RepositoryErrorKind.COMMAND,
                operation = "Request",
                message = "failure $index",
                deviceId = null,
                deviceName = null,
                severity = RepositoryErrorSeverity.TRANSIENT,
                timestampMs = System.currentTimeMillis(),
            )
        }

        // Bounded and newest first: only the newest ERROR_LIMIT entries survive.
        assertEquals(20, collector.errors.value.size)
        assertEquals("failure 24", collector.errors.value.first().message)

        collector.dismissError(collector.errors.value.first().id)
        assertEquals(19, collector.errors.value.size)
        assertEquals("failure 23", collector.errors.value.first().message)

        collector.clearErrors()
        assertTrue(collector.errors.value.isEmpty())
    }

    @Test
    fun clearErrorsByKindRemovesOnlyMatchingErrors() = runBlocking {
        val collector = CoreEventCollector()
        collector.recordError(
            kind = RepositoryErrorKind.CORE,
            operation = "Core startup",
            message = "ClipSync failed to start",
            deviceId = null,
            deviceName = null,
            severity = RepositoryErrorSeverity.PERSISTENT,
            timestampMs = System.currentTimeMillis(),
        )
        collector.recordError(
            kind = RepositoryErrorKind.COMMAND,
            operation = "Pairing request",
            message = "pairing failed",
            deviceId = null,
            deviceName = null,
            severity = RepositoryErrorSeverity.TRANSIENT,
            timestampMs = System.currentTimeMillis(),
        )

        assertEquals(2, collector.errors.value.size)

        // A recovered condition clears its own kind and nothing else.
        collector.clearErrors(RepositoryErrorKind.CORE)
        assertEquals(1, collector.errors.value.size)
        assertEquals(RepositoryErrorKind.COMMAND, collector.errors.value.single().kind)
    }

    @Test
    fun recentClipboardTracksTheNewestHistoryEntry() = runBlocking {
        val collector = CoreEventCollector()
        val newest = historyItem("newest")
        val older = historyItem("older")

        collector.seedSnapshot(
            identity = null,
            trusted = emptyList(),
            discovered = emptyList(),
            pairing = emptyList(),
            history = listOf(newest, older),
            sync = SyncState(),
        )

        val recent = withTimeout(5_000) { collector.recentClipboard.first { it != null } }
        assertEquals(newest, recent)
        // Synchronous with the history write: no refresh window in which the list
        // has moved on but recentClipboard has not.
        assertEquals(newest, collector.recentClipboard.value)

        // A targeted refresh replaces the list, and the derived value follows.
        collector.setHistory(listOf(older))
        assertEquals(older, collector.recentClipboard.value)

        collector.setHistory(emptyList())
        assertNull(collector.recentClipboard.value)
    }

    @Test
    fun identityIsLoadedFromTheCoreSnapshot() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val storage = context.filesDir
            .resolve("clipsync-m7-identity-${System.nanoTime()}.db")
            .absolutePath
        val coreManager = CoreManager(context, config(storage))

        coreManager.start()

        // The predicate guarantees non-null; the assertion keeps the type non-null.
        val identity = withTimeout(10_000) { coreManager.identity.first { it != null } }!!
        assertTrue(identity.deviceId.isNotBlank())
        assertTrue(identity.deviceName.isNotBlank())

        coreManager.stop()
    }

    @Test
    fun commandFailureIsRecordedBeforeItIsRethrown() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val coreManager = CoreManager(
            context,
            config(context.filesDir.resolve("clipsync-m7-unstarted.db").absolutePath),
        )

        // Deliberately never started: the repository refuses the command, but the
        // failure still has to reach the errors list rather than being swallowed.
        var thrown = false
        runCatching { coreManager.requestPairing("some-device") }
            .onFailure { thrown = true }

        assertTrue(thrown)
        val error = coreManager.errors.value.single()
        assertEquals(RepositoryErrorKind.COMMAND, error.kind)
        assertEquals("Pairing request", error.operation)
        assertEquals(RepositoryErrorSeverity.TRANSIENT, error.severity)

        coreManager.dismissError(error.id)
        assertTrue(coreManager.errors.value.isEmpty())
    }
}
