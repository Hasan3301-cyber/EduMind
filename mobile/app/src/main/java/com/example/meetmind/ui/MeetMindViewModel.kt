package com.example.meetmind.ui

import android.app.Application
import androidx.core.content.ContextCompat
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.example.meetmind.config.ApiConfig
import com.example.meetmind.data.TranscriptQueue
import com.example.meetmind.model.MeetMindScreen
import com.example.meetmind.model.ProcessResponse
import com.example.meetmind.model.RecordingState
import com.example.meetmind.service.RecordingService
import com.example.meetmind.service.RecordingStateBus
import com.example.meetmind.service.TranscriptSyncWorker
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class MeetMindUiState(
    val screen: MeetMindScreen = MeetMindScreen.Login,
    val recordingState: RecordingState = RecordingState.Idle,
    val queuedCount: Int = 0,
    val lastResult: ProcessResponse? = null,
    val configurationErrors: List<String> = emptyList()
)

class MeetMindViewModel(application: Application) : AndroidViewModel(application) {
    private val queue = TranscriptQueue(application)
    private val mutableUiState = MutableStateFlow(
        MeetMindUiState(
            queuedCount = queue.count(),
            lastResult = queue.latestResult(),
            configurationErrors = ApiConfig.validationErrors()
        )
    )
    val uiState: StateFlow<MeetMindUiState> = mutableUiState.asStateFlow()

    init {
        viewModelScope.launch {
            RecordingStateBus.state.collectLatest { recordingState ->
                mutableUiState.update { current ->
                    current.copy(
                        recordingState = recordingState,
                        queuedCount = queue.count(),
                        lastResult = queue.latestResult()
                    )
                }
            }
        }
        if (queue.count() > 0) {
            TranscriptSyncWorker.enqueue(application)
        }
    }

    fun continueToDashboard() {
        mutableUiState.update { current -> current.copy(screen = MeetMindScreen.Dashboard) }
    }

    fun showResult() {
        mutableUiState.update { current ->
            current.copy(screen = MeetMindScreen.Result, lastResult = queue.latestResult())
        }
    }

    fun showDashboard() {
        mutableUiState.update { current -> current.copy(screen = MeetMindScreen.Dashboard) }
    }

    fun startRecording() {
        val errors = ApiConfig.validationErrors()
        if (errors.isNotEmpty()) {
            publishError(errors.joinToString(" "))
            return
        }
        ContextCompat.startForegroundService(
            getApplication(),
            RecordingService.startIntent(getApplication())
        )
    }

    fun stopRecording() {
        getApplication<Application>().startService(RecordingService.stopIntent(getApplication()))
    }

    fun retryPendingSync() {
        if (queue.count() > 0) {
            queue.resetFailures()
            TranscriptSyncWorker.enqueue(getApplication())
            RecordingStateBus.publish(RecordingState.Processing("Retrying queued lecture transcripts."))
        }
        refresh()
    }

    fun reportPermissionError() {
        publishError("Microphone permission is required to record a lecture.")
    }

    fun refresh() {
        mutableUiState.update { current ->
            current.copy(
                queuedCount = queue.count(),
                lastResult = queue.latestResult(),
                configurationErrors = ApiConfig.validationErrors()
            )
        }
    }

    private fun publishError(message: String) {
        RecordingStateBus.publish(RecordingState.Error(message))
        mutableUiState.update { current ->
            current.copy(configurationErrors = ApiConfig.validationErrors())
        }
    }
}
