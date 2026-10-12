package com.abram056.clipsync

import android.Manifest
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import com.abram056.clipsync.ui.ClipSyncApp
import com.abram056.clipsync.ui.theme.ClipSyncTheme

class MainActivity : ComponentActivity() {

    private val viewModel: ClipSyncViewModel by viewModels()

    private val requestNotificationPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            requestNotificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        // Idempotent request: activity creation or recreation reuses the
        // application-scoped core and foreground service rather than creating
        // another one.
        viewModel.ensureStarted()
        setContent {
            ClipSyncTheme {
                ClipSyncApp()
            }
        }
    }
}
