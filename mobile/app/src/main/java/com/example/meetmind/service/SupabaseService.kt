package com.example.meetmind.service

import com.example.meetmind.config.ApiConfig
import com.example.meetmind.model.SyncResult
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.TimeUnit

class SupabaseService {
    private val client = OkHttpClient.Builder()
        .connectTimeout(30, TimeUnit.SECONDS)
        .readTimeout(45, TimeUnit.SECONDS)
        .writeTimeout(45, TimeUnit.SECONDS)
        .build()

    suspend fun syncTranscript(
        transcript: String,
        createdAt: String,
        assemblyTranscriptId: String
    ): SyncResult = withContext(Dispatchers.IO) {
        if (ApiConfig.usesDirectGateway) {
            syncToEduMind(transcript, createdAt, assemblyTranscriptId)
        } else {
            syncToSupabase(transcript, createdAt)
        }
    }

    private fun syncToEduMind(
        transcript: String,
        createdAt: String,
        assemblyTranscriptId: String
    ): SyncResult {
        val payload = JSONObject().apply {
            put("content", transcript)
            put("content_type", "transcript")
            put("scope", "module")
            put(
                "metadata",
                JSONObject().apply {
                    put("source", "meetmind")
                    put("created_at", createdAt)
                    put("assemblyai_transcript_id", assemblyTranscriptId)
                }
            )
        }
        val request = Request.Builder()
            .url("${ApiConfig.eduMindGatewayUrl}/api/v1/modules/class-notes/memory/store")
            .header("content-type", "application/json")
            .apply {
                if (ApiConfig.eduMindGatewayToken.isNotBlank()) {
                    header("authorization", "Bearer ${ApiConfig.eduMindGatewayToken}")
                }
            }
            .post(payload.toString().toRequestBody(JSON_MEDIA_TYPE))
            .build()
        val response = executeJson(request, "EduMind gateway")
        return SyncResult(
            target = "edumind-gateway",
            remoteId = response.optJSONObject("record")?.optString("id")?.takeIf(String::isNotBlank)
        )
    }

    private fun syncToSupabase(transcript: String, createdAt: String): SyncResult {
        if (ApiConfig.supabaseUrl.isBlank() || ApiConfig.supabaseAnonKey.isBlank()) {
            throw ConfigurationException("Supabase settings are missing.")
        }
        val payload = JSONArray().put(
            JSONObject().apply {
                put("transcript", transcript)
                put("created_at", createdAt)
            }
        )
        val request = Request.Builder()
            .url("${ApiConfig.supabaseUrl}/rest/v1/meetings")
            .header("apikey", ApiConfig.supabaseAnonKey)
            .header("authorization", "Bearer ${ApiConfig.supabaseAnonKey}")
            .header("Prefer", "return=representation")
            .header("content-type", "application/json")
            .post(payload.toString().toRequestBody(JSON_MEDIA_TYPE))
            .build()
        val response = executeJson(request, "Supabase")
        return SyncResult(
            target = "supabase",
            remoteId = response.optJSONObject("first")?.optString("id")?.takeIf(String::isNotBlank)
        )
    }

    private fun executeJson(request: Request, serviceName: String): JSONObject {
        val body = client.newCall(request).execute().use { response ->
            val content = response.body?.string().orEmpty()
            if (!response.isSuccessful) {
                if (response.code in 400..499) {
                    throw PermanentSyncException("$serviceName rejected the transcript (${response.code}).")
                }
                throw RetryableSyncException("$serviceName is unavailable (${response.code}).")
            }
            content
        }
        if (body.length > MAX_RESPONSE_CHARACTERS) {
            throw RetryableSyncException("$serviceName returned an oversized response.")
        }
        return if (body.trim().startsWith("[")) {
            val array = runCatching { JSONArray(body) }
                .getOrElse { throw RetryableSyncException("$serviceName returned invalid JSON.", it) }
            JSONObject().put("items", array).also { objectValue ->
                array.optJSONObject(0)?.let { first -> objectValue.put("first", first) }
            }
        } else {
            runCatching { JSONObject(body) }
                .getOrElse { throw RetryableSyncException("$serviceName returned invalid JSON.", it) }
        }
    }

    private companion object {
        val JSON_MEDIA_TYPE = "application/json; charset=utf-8".toMediaType()
        const val MAX_RESPONSE_CHARACTERS = 1_000_000
    }
}
