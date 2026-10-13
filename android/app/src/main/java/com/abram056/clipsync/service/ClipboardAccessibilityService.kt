package com.abram056.clipsync.service

import android.accessibilityservice.AccessibilityService
import android.content.ClipboardManager
import android.content.Context
import android.view.accessibility.AccessibilityEvent
import com.abram056.clipsync.ClipSyncApplication
import com.abram056.clipsync.core.CoreManager

/// Minimal accessibility-based clipboard capture for API ≥ 29 (doc 13 §12).
///
/// Privacy (doc 13 §21/§44): reads clipboard text only on qualifying events —
/// never inspects, transmits, or logs window or screen content — even though
/// the manifest grants `canRetrieveWindowContent` for clipboard access
/// (doc 09 §89). Forwarding happens only while the core is running: capture
/// events never start sync themselves, and user-facing enablement plus the
/// Settings toggle arrive with Milestone 10 (doc 15 §9).
class ClipboardAccessibilityService : AccessibilityService() {

    private var manager: CoreManager? = null

    private val filter = ClipboardFilter(
        onForward = { content ->
            // Throws when the core is stopped; the filter swallows it, records
            // nothing, and allows a later identical copy to retry.
            manager?.onClipboardChanged(content)
        },
    )

    override fun onCreate() {
        super.onCreate()
        manager = (application as ClipSyncApplication).coreManager
        filter.start()
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        when (event?.eventType) {
            // "Content and focus events" from doc 05/§Phase E: window focus
            // transitions surface as TYPE_WINDOW_STATE_CHANGED (there is no
            // TYPE_WINDOW_FOCUS_CHANGED constant).
            AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED,
            AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED,
            -> filter.submit(currentClipboardText())

            else -> Unit
        }
    }

    /// First text item only; never coerces URIs or files (doc 13 §21).
    private fun currentClipboardText(): CharSequence? {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = clipboard.primaryClip ?: return null
        return if (clip.itemCount > 0) clip.getItemAt(0).text else null
    }

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        filter.stop()
        manager = null
        super.onDestroy()
    }
}
