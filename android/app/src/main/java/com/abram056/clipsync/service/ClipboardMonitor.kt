package com.abram056.clipsync.service

import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import com.abram056.clipsync.data.ClipSyncRepository
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/// Maximum forwarded clipboard payload — mirrors the core's configured
/// `max_clipboard_bytes` (doc 08, doc 13 §20).
private const val MAX_CLIPBOARD_BYTES = 2 * 1024 * 1024

/// Trailing debounce that coalesces bursts of capture callbacks (doc 13 §19).
private const val DEBOUNCE_MS = 250L

/// Trailing poll interval for the OEM fallback path (see
/// [PrimaryClipClipboardMonitor]). Some devices never dispatch the platform
/// clip-changed listener, so the monitor also notices changes by reading the
/// primary clip at this cadence while it is attached.
private const val POLL_INTERVAL_MS = 500L

/// Stable capture boundary for the hybrid clipboard mechanism (doc 13 §12).
///
/// One implementation serves API ≤ 28 (platform listener plus a polling
/// fallback — see [PrimaryClipClipboardMonitor]); on API ≥ 29 capture comes
/// from the user-enabled [ClipboardAccessibilityService], so [create] returns
/// a no-op and the rest of the architecture is unchanged. Lifecycle: the
/// foreground service attaches the monitor only after the core is running
/// (doc 13 §33) and detaches it before the core goes away (§34).
interface ClipboardMonitor {
    fun start()
    fun stop()

    companion object {
        fun create(context: Context, repository: ClipSyncRepository): ClipboardMonitor =
            if (Build.VERSION.SDK_INT <= Build.VERSION_CODES.P) {
                PrimaryClipClipboardMonitor(context.applicationContext, repository)
            } else {
                NoOpClipboardMonitor
            }
    }
}

/// Shared capture gate in front of both clipboard mechanisms (doc 13 §19–21).
///
/// Keeps only non-empty text within the core's size budget, coalesces burst
/// callbacks with a short trailing debounce, and forwards at most one
/// submission of unchanged content. A forwarding failure (for example, a
/// capture arriving while the core is stopped) records nothing, so a later
/// identical copy retries; Rust deduplication stays authoritative (§18).
class ClipboardFilter(
    private val onForward: suspend (String) -> Unit,
    private val maxBytes: Int = MAX_CLIPBOARD_BYTES,
    private val debounceMs: Long = DEBOUNCE_MS,
) {

    private var scope: CoroutineScope? = null
    private var pending: Job? = null
    private var lastForwarded: String? = null

    fun start() {
        if (scope != null) return
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    }

    fun stop() {
        pending?.cancel()
        pending = null
        scope?.cancel()
        scope = null
        lastForwarded = null
    }

    /// Queue clipboard text for forwarding; returns immediately. Submissions
    /// while stopped are dropped.
    fun submit(text: CharSequence?) {
        val content = text?.toString().orEmpty()
        if (content.isEmpty() || !withinSizeBudget(content)) return
        val scope = scope ?: return
        pending?.cancel()
        pending = scope.launch {
            delay(debounceMs)
            if (content == lastForwarded) return@launch
            try {
                onForward(content)
                lastForwarded = content
            } catch (error: CancellationException) {
                throw error
            } catch (@Suppress("SwallowedException") error: Exception) {
                // Forward failed (core stopped); records nothing so a later
                // identical copy retries.
            }
        }
    }

    /// Early size guard (doc 13 §20) without forwarding oversized content
    /// across the FFI boundary; Rust remains the authoritative validator.
    private fun withinSizeBudget(content: String): Boolean =
        // UTF-8 size is never smaller than the UTF-16 length, so the cheap
        // check rejects ASCII pastes exactly before any byte conversion.
        content.length <= maxBytes &&
            content.toByteArray(Charsets.UTF_8).size <= maxBytes
}

/// Clipboard capture for API ≤ 28 via
/// [ClipboardManager.OnPrimaryClipChangedListener] (doc 13 §12).
///
/// The platform listener is unreliable on some OEM builds — notably Huawei
/// EMUI 8, which never dispatches the callback at all — so a low-frequency
/// poll of the primary clip backs it up. Both paths converge on the same
/// [ClipboardFilter]; its debounce and skip-unchanged gate make the overlap
/// idempotent. The poll baselines the clip present at [start] and only
/// forwards later differences, matching listener semantics (pre-existing
/// clipboard content is not a local copy event).
private class PrimaryClipClipboardMonitor(
    context: Context,
    repository: ClipSyncRepository,
) : ClipboardMonitor {

    private val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    private val filter = ClipboardFilter(onForward = repository::onClipboardChanged)
    private var scope: CoroutineScope? = null
    private var running = false

    private val listener = ClipboardManager.OnPrimaryClipChangedListener {
        filter.submit(currentText())
    }

    /// First text item only; never coerces URIs or files (doc 13 §21).
    private fun currentText(): CharSequence? {
        val clip = clipboard.primaryClip ?: return null
        return if (clip.itemCount > 0) clip.getItemAt(0).text else null
    }

    override fun start() {
        if (running) return
        running = true
        filter.start()
        clipboard.addPrimaryClipChangedListener(listener)
        // Runs on Dispatchers.Main.immediate so poll submissions serialize
        // with listener callbacks (the filter is main-confined).
        val pollScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
        scope = pollScope
        var lastSeen = currentText()?.toString()
        pollScope.launch {
            while (true) {
                delay(POLL_INTERVAL_MS)
                val text = currentText()?.toString()
                if (text != lastSeen) {
                    lastSeen = text
                    filter.submit(text)
                }
            }
        }
    }

    override fun stop() {
        if (!running) return
        running = false
        clipboard.removePrimaryClipChangedListener(listener)
        scope?.cancel()
        scope = null
        filter.stop()
    }
}

/// API ≥ 29 placeholder: capture is performed by the system-enabled
/// [ClipboardAccessibilityService] instead of a process-owned listener.
private object NoOpClipboardMonitor : ClipboardMonitor {
    override fun start() = Unit
    override fun stop() = Unit
}
