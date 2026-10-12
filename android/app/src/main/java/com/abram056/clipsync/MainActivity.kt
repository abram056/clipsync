package com.abram056.clipsync

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import com.abram056.clipsync.ui.ClipSyncApp
import com.abram056.clipsync.ui.theme.ClipSyncTheme

class MainActivity : ComponentActivity() {

    private val viewModel: ClipSyncViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // Idempotent request: activity creation or recreation reuses the
        // application-scoped core rather than creating another one.
        viewModel.ensureStarted()
        setContent {
            ClipSyncTheme {
                ClipSyncApp()
            }
        }
    }
}
