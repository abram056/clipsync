package com.abram056.clipsync.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.graphics.Color

class ClipSyncExtraColors(
    val success: Color,
    val warning: Color,
)

val LocalClipSyncExtraColors = compositionLocalOf {
    ClipSyncExtraColors(
        success = Color.Unspecified,
        warning = Color.Unspecified,
    )
}

private val LightColorScheme = lightColorScheme(
    primary = Color(0xFF2E6B5E),
    onPrimary = Color(0xFFFFFFFF),
    secondary = Color(0xFF4A6572),
    onSecondary = Color(0xFFFFFFFF),
    background = Color(0xFFF7F9F8),
    onBackground = Color(0xFF1A1C1B),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF1A1C1B),
    error = Color(0xFFB3261E),
    onError = Color(0xFFFFFFFF),
    outline = Color(0xFF79747E),
)

private val DarkColorScheme = darkColorScheme(
    primary = Color(0xFF7FD1C1),
    onPrimary = Color(0xFF00332B),
    secondary = Color(0xFFABC7D9),
    onSecondary = Color(0xFF0B2A38),
    background = Color(0xFF101413),
    onBackground = Color(0xFFE1E3E1),
    surface = Color(0xFF1A1E1D),
    onSurface = Color(0xFFE1E3E1),
    error = Color(0xFFF2B8B5),
    onError = Color(0xFF601410),
    outline = Color(0xFF8C898E),
)

private val LightExtraColors = ClipSyncExtraColors(
    success = Color(0xFF2E7D32),
    warning = Color(0xFFB26A00),
)

private val DarkExtraColors = ClipSyncExtraColors(
    success = Color(0xFF81C784),
    warning = Color(0xFFFFB74D),
)

@Composable
fun ClipSyncTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    CompositionLocalProvider(
        LocalClipSyncExtraColors provides if (darkTheme) DarkExtraColors else LightExtraColors,
    ) {
        MaterialTheme(
            colorScheme = if (darkTheme) DarkColorScheme else LightColorScheme,
            content = content,
        )
    }
}
