package com.abram056.clipsync.service

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/// Applies remote clipboard updates: String → Android clipboard (doc 13 §15–16).
///
/// Writes only — it never reads the clipboard (background reads are restricted
/// on API 29+ anyway) and knows nothing about Rust, peers, history, or
/// synchronization. The resulting local change is observed naturally by the
/// capture path; the Rust core's replay cache / deduplication prevents a
/// rebroadcast loop (doc 13 §18, doc 12 §13).
class ClipboardWriter(context: Context) {

    private val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

    /// Write on the main thread (doc 09 §Feed path). No label: the write must
    /// look exactly like a normal copy and carries no correctness-bearing
    /// metadata (doc 13 §17).
    suspend fun write(content: String) = withContext(Dispatchers.Main) {
        clipboard.setPrimaryClip(ClipData.newPlainText(null, content))
    }
}
