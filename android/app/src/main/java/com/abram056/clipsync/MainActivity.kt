package com.abram056.clipsync

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import com.abram056.clipsync.ui.ClipSyncApp
import com.abram056.clipsync.ui.theme.ClipSyncTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            ClipSyncTheme {
                ClipSyncApp()
            }
        }
    }
}
