package com.example.meetmind.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@Composable
fun LoginScreen(configurationErrors: List<String>, onContinue: () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        Text("MeetMind", style = MaterialTheme.typography.displaySmall)
        Text(
            "Capture a lecture, create a speaker-labelled transcript, and place it in EduMind Class Notes.",
            style = MaterialTheme.typography.bodyLarge
        )
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                Text("Private by default", style = MaterialTheme.typography.titleMedium)
                Text(
                    "Keys are injected during the build. Audio stays on this device until the queued sync worker uploads it over HTTPS.",
                    style = MaterialTheme.typography.bodyMedium
                )
            }
        }
        if (configurationErrors.isNotEmpty()) {
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    Text("Setup required", style = MaterialTheme.typography.titleMedium)
                    configurationErrors.forEach { error ->
                        Text("• $error", style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
        }
        Button(modifier = Modifier.fillMaxWidth(), onClick = onContinue) {
            Text("Open lecture workspace")
        }
    }
}
