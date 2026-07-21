package com.example.meetmind.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.example.meetmind.model.ProcessResponse

@Composable
fun ResultScreen(result: ProcessResponse?, onBack: () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        Text("Latest sync", style = MaterialTheme.typography.headlineMedium)
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                Text(
                    result?.message ?: "No completed transcript is stored on this device yet.",
                    style = MaterialTheme.typography.bodyLarge
                )
                result?.syncTarget?.let { target ->
                    Text("Destination: $target", style = MaterialTheme.typography.bodyMedium)
                }
                result?.createdAt?.let { timestamp ->
                    Text("Recorded: $timestamp", style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
        result?.transcript?.let { transcript ->
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Text("Local preview", style = MaterialTheme.typography.titleMedium)
                    Text(
                        transcript,
                        style = MaterialTheme.typography.bodyMedium
                    )
                }
            }
        }
        Button(onClick = onBack) {
            Text("Back to lecture workspace")
        }
    }
}
