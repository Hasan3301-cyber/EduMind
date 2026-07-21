package com.example.meetmind.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.example.meetmind.model.RecordingState

@Composable
fun DashboardScreen(
    recordingState: RecordingState,
    queuedCount: Int,
    onStartRecording: () -> Unit,
    onStopRecording: () -> Unit,
    onOpenResult: () -> Unit,
    onRetryPending: () -> Unit
) {
    val recording = recordingState is RecordingState.Recording
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        Text("Lecture workspace", style = MaterialTheme.typography.headlineMedium)
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                Text(statusTitle(recordingState), style = MaterialTheme.typography.titleLarge)
                Text(statusDetail(recordingState), style = MaterialTheme.typography.bodyMedium)
                Button(
                    modifier = Modifier.fillMaxWidth(),
                    onClick = if (recording) onStopRecording else onStartRecording
                ) {
                    Text(if (recording) "Stop and queue transcript" else "Record lecture")
                }
            }
        }
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp)
            ) {
                Text("Offline queue", style = MaterialTheme.typography.titleMedium)
                Text(
                    "$queuedCount recording${if (queuedCount == 1) "" else "s"} waiting for a network-safe transcript sync.",
                    style = MaterialTheme.typography.bodyMedium
                )
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    OutlinedButton(onClick = onRetryPending, enabled = queuedCount > 0) {
                        Text("Retry sync")
                    }
                    OutlinedButton(onClick = onOpenResult) {
                        Text("Latest result")
                    }
                }
            }
        }
    }
}

private fun statusTitle(state: RecordingState): String = when (state) {
    RecordingState.Idle -> "Ready to record"
    is RecordingState.Recording -> "Recording lecture"
    is RecordingState.Queued -> "Queued safely"
    is RecordingState.Processing -> "Processing transcript"
    is RecordingState.Synced -> "Transcript synced"
    is RecordingState.Error -> "Needs attention"
}

private fun statusDetail(state: RecordingState): String = when (state) {
    RecordingState.Idle -> "MeetMind keeps the microphone alive in a foreground service while you record."
    is RecordingState.Recording -> "The app can remain in the background while the microphone service records."
    is RecordingState.Queued -> state.message
    is RecordingState.Processing -> state.message
    is RecordingState.Synced -> state.message
    is RecordingState.Error -> state.message
}
