package com.example.meetmind.model

data class TranscriptRequest(
    val audioUrl: String,
    val speechModels: List<String> = listOf("universal-2"),
    val languageCodes: List<String> = listOf("en", "bn"),
    val speakerLabels: Boolean = true,
    val punctuate: Boolean = true,
    val formatText: Boolean = true
)

data class TranscriptResponse(
    val id: String,
    val status: String,
    val text: String,
    val utterances: List<TranscriptUtterance> = emptyList(),
    val error: String? = null
) {
    fun formattedText(): String = utterances
        .takeIf { values -> values.isNotEmpty() }
        ?.joinToString("\n") { utterance -> "Speaker ${utterance.speaker}: ${utterance.text}" }
        ?: text
}

data class TranscriptUtterance(
    val speaker: String,
    val text: String
)

data class ProcessResponse(
    val success: Boolean,
    val message: String,
    val transcript: String? = null,
    val syncTarget: String? = null,
    val remoteId: String? = null,
    val createdAt: String? = null
)

data class AnalysisContainer(
    val summary: String,
    val details: List<AnalysisDetail> = emptyList()
)

data class AnalysisDetail(
    val title: String,
    val content: String
)

data class ActionItem(
    val title: String,
    val completed: Boolean = false
)

data class AnalyzeTextRequest(
    val text: String,
    val language: String = "en"
)

data class PendingTranscript(
    val id: String,
    val audioPath: String,
    val createdAt: String,
    val attempts: Int = 0,
    val lastError: String? = null
)

data class SyncResult(
    val target: String,
    val remoteId: String? = null
)

enum class MeetMindScreen {
    Login,
    Dashboard,
    Result
}

sealed interface RecordingState {
    data object Idle : RecordingState
    data class Recording(val startedAt: String) : RecordingState
    data class Queued(val message: String) : RecordingState
    data class Processing(val message: String) : RecordingState
    data class Synced(val message: String) : RecordingState
    data class Error(val message: String) : RecordingState
}
