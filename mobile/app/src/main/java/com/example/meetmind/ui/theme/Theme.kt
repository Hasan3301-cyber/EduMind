package com.example.meetmind.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable

private val DarkColors = darkColorScheme(
    primary = Indigo,
    secondary = Mint,
    background = Ink
)

private val LightColors = lightColorScheme(
    primary = Indigo,
    secondary = Mint,
    background = Cloud,
    onBackground = Ink,
    onSurfaceVariant = Slate
)

@Composable
fun MeetMindTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) DarkColors else LightColors,
        content = content
    )
}
