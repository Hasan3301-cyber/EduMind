package com.example.meetmind.service

import com.example.meetmind.model.TranscriptRequest
import com.example.meetmind.model.TranscriptResponse
import com.example.meetmind.model.TranscriptUtterance
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.asRequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.util.concurrent.TimeUnit

class ConfigurationException(message: String) : IllegalStateException(message)

class PermanentSyncException(message: String) : IllegalArgumentException(message)

class RetryableSyncException(message: String, cause: Throwable? = null) : IOException(message, cause)

class AssemblyAiService(private val apiKey: String) {
    private val client = OkHttpClient.Builder()
        .connectTimeout(30, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .writeTimeout(60, TimeUnit.SECONDS)
        .callTimeout(2, TimeUnit.MINUTES)
        .build()

    suspend fun transcribe(audioFile: File): TranscriptResponse = withContext(Dispatchers.IO) {
        if (apiKey.isBlank()) {
            throw ConfigurationException("ASSEMBLYAI_API_KEY is missing.")
        }
        if (!audioFile.isFile) {
            throw PermanentSyncException("Recorded audio is unavailable.")
        }
        if (audioFile.length() > MAX_AUDIO_BYTES) {
            throw PermanentSyncException("Recorded audio exceeds the 100 MiB upload limit.")
        }

        val audioUrl = upload(audioFile)
        val transcriptId = submit(TranscriptRequest(audioUrl))
        poll(transcriptId)
    }

    private fun upload(audioFile: File): String {
        val request = Request.Builder()
            .url(UPLOAD_URL)
            .header("authorization", apiKey)
            .post(audioFile.asRequestBody(AUDIO_MEDIA_TYPE))
            .build()
        return executeJson(request).optString("upload_url").takeIf(String::isNotBlank)
            ?: throw RetryableSyncException("AssemblyAI did not return an upload URL.")
    }

    private fun submit(input: TranscriptRequest): String {
        val payload = JSONObject().apply {
            put("audio_url", input.audioUrl)
            put("speech_models", stringArray(input.speechModels))
            put("language_codes", stringArray(input.languageCodes))
            put("speaker_labels", input.speakerLabels)
            put("punctuate", input.punctuate)
            put("format_text", input.formatText)
        }
        val request = Request.Builder()
            .url(TRANSCRIPT_URL)
            .header("authorization", apiKey)
            .header("content-type", "application/json")
            .post(payload.toString().toRequestBody(JSON_MEDIA_TYPE))
            .build()
        return executeJson(request).optString("id").takeIf(String::isNotBlank)
            ?: throw RetryableSyncException("AssemblyAI did not return a transcript ID.")
    }

    private suspend fun poll(transcriptId: String): TranscriptResponse {
        repeat(MAX_POLL_ATTEMPTS) {
            delay(POLL_INTERVAL_MILLIS)
            val request = Request.Builder()
                .url("$TRANSCRIPT_URL/$transcriptId")
                .header("authorization", apiKey)
                .build()
            val response = parseResponse(executeJson(request))
            when (response.status.lowercase()) {
                "completed" -> return response
                "error" -> throw PermanentSyncException(response.error ?: "AssemblyAI could not transcribe this recording.")
            }
        }
        throw RetryableSyncException("AssemblyAI transcript polling timed out.")
    }

    private fun parseResponse(payload: JSONObject): TranscriptResponse {
        val utterances = payload.optJSONArray("utterances")?.let { values ->
            buildList {
                repeat(values.length()) { index ->
                    val utterance = values.optJSONObject(index) ?: return@repeat
                    val text = utterance.optString("text").trim()
                    if (text.isNotBlank()) {
                        add(
                            TranscriptUtterance(
                                speaker = utterance.optString("speaker", "A"),
                                text = text
                            )
                        )
                    }
                }
            }
        }.orEmpty()
        return TranscriptResponse(
            id = payload.optString("id"),
            status = payload.optString("status"),
            text = payload.optString("text"),
            utterances = utterances,
            error = payload.optString("error").takeIf(String::isNotBlank)
        )
    }

    private fun executeJson(request: Request): JSONObject {
        val body = client.newCall(request).execute().use { response ->
            val content = response.body?.string().orEmpty()
            if (!response.isSuccessful) {
                if (response.code in 400..499) {
                    throw PermanentSyncException("AssemblyAI rejected the request (${response.code}).")
                }
                throw RetryableSyncException("AssemblyAI is unavailable (${response.code}).")
            }
            content
        }
        if (body.length > MAX_RESPONSE_CHARACTERS) {
            throw RetryableSyncException("AssemblyAI returned an oversized response.")
        }
        return runCatching { JSONObject(body) }
            .getOrElse { throw RetryableSyncException("AssemblyAI returned invalid JSON.", it) }
    }

    private fun stringArray(values: List<String>): JSONArray = JSONArray().apply {
        values.forEach(::put)
    }

    private companion object {
        val AUDIO_MEDIA_TYPE = "audio/mp4".toMediaType()
        val JSON_MEDIA_TYPE = "application/json; charset=utf-8".toMediaType()
        const val UPLOAD_URL = "https://api.assemblyai.com/v2/upload"
        const val TRANSCRIPT_URL = "https://api.assemblyai.com/v2/transcript"
        const val MAX_AUDIO_BYTES = 100 * 1024 * 1024L
        const val MAX_RESPONSE_CHARACTERS = 1_000_000
        const val MAX_POLL_ATTEMPTS = 120
        const val POLL_INTERVAL_MILLIS = 2_000L
    }
}
