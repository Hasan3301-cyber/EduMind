package com.example.meetmind.data

import android.content.Context
import com.example.meetmind.model.PendingTranscript
import com.example.meetmind.model.ProcessResponse
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.time.Instant
import java.util.UUID

class TranscriptQueue(context: Context) {
    private val preferences = context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)
    private val lock = Any()

    fun enqueue(audioFile: File): PendingTranscript = synchronized(lock) {
        val pending = loadPending()
        val item = PendingTranscript(
            id = UUID.randomUUID().toString(),
            audioPath = audioFile.absolutePath,
            createdAt = Instant.now().toString()
        )
        pending += item
        savePending(pending)
        item
    }

    fun next(): PendingTranscript? = synchronized(lock) {
        loadPending().minByOrNull(PendingTranscript::createdAt)
    }

    fun count(): Int = synchronized(lock) {
        loadPending().size
    }

    fun remove(id: String) = synchronized(lock) {
        savePending(loadPending().filterNot { item -> item.id == id })
    }

    fun recordFailure(id: String, message: String) = synchronized(lock) {
        val updated = loadPending().map { item ->
            if (item.id == id) {
                item.copy(attempts = item.attempts + 1, lastError = message.take(MAX_ERROR_LENGTH))
            } else {
                item
            }
        }
        savePending(updated)
    }

    fun resetFailures() = synchronized(lock) {
        savePending(
            loadPending().map { item ->
                item.copy(attempts = 0, lastError = null)
            }
        )
    }

    fun saveResult(result: ProcessResponse) = synchronized(lock) {
        val serialized = JSONObject().apply {
            put("success", result.success)
            put("message", result.message)
            putNullable("transcript", result.transcript?.let(::transcriptPreview))
            putNullable("syncTarget", result.syncTarget)
            putNullable("remoteId", result.remoteId)
            putNullable("createdAt", result.createdAt)
        }
        preferences.edit().putString(LAST_RESULT_KEY, serialized.toString()).apply()
    }

    fun latestResult(): ProcessResponse? = synchronized(lock) {
        val serialized = preferences.getString(LAST_RESULT_KEY, null) ?: return null
        runCatching { JSONObject(serialized) }.getOrNull()?.let { value ->
            ProcessResponse(
                success = value.optBoolean("success"),
                message = value.optString("message"),
                transcript = value.optionalString("transcript"),
                syncTarget = value.optionalString("syncTarget"),
                remoteId = value.optionalString("remoteId"),
                createdAt = value.optionalString("createdAt")
            )
        }
    }

    private fun loadPending(): MutableList<PendingTranscript> {
        val serialized = preferences.getString(PENDING_KEY, "[]") ?: "[]"
        val jsonArray = runCatching { JSONArray(serialized) }.getOrElse { JSONArray() }
        return buildList {
            repeat(jsonArray.length()) { index ->
                val value = jsonArray.optJSONObject(index) ?: return@repeat
                val id = value.optString("id")
                val audioPath = value.optString("audioPath")
                val createdAt = value.optString("createdAt")
                if (id.isNotBlank() && audioPath.isNotBlank() && createdAt.isNotBlank()) {
                    add(
                        PendingTranscript(
                            id = id,
                            audioPath = audioPath,
                            createdAt = createdAt,
                            attempts = value.optInt("attempts"),
                            lastError = value.optionalString("lastError")
                        )
                    )
                }
            }
        }.toMutableList()
    }

    private fun savePending(items: List<PendingTranscript>) {
        val serialized = JSONArray().apply {
            items.forEach { item ->
                put(
                    JSONObject().apply {
                        put("id", item.id)
                        put("audioPath", item.audioPath)
                        put("createdAt", item.createdAt)
                        put("attempts", item.attempts)
                        putNullable("lastError", item.lastError)
                    }
                )
            }
        }
        preferences.edit().putString(PENDING_KEY, serialized.toString()).apply()
    }

    private fun JSONObject.putNullable(name: String, value: String?) {
        put(name, value ?: JSONObject.NULL)
    }

    private fun JSONObject.optionalString(name: String): String? =
        optString(name).takeIf { value -> value.isNotBlank() && value != "null" }

    private fun transcriptPreview(value: String): String {
        val normalized = value.trim()
        if (normalized.length <= MAX_RESULT_TRANSCRIPT_CHARS) {
            return normalized
        }
        return normalized.take(MAX_RESULT_TRANSCRIPT_CHARS - 1).trimEnd() + "…"
    }

    private companion object {
        const val PREFERENCES_NAME = "meetmind_transcript_queue"
        const val PENDING_KEY = "pending"
        const val LAST_RESULT_KEY = "last_result"
        const val MAX_ERROR_LENGTH = 500
        const val MAX_RESULT_TRANSCRIPT_CHARS = 1_200
    }
}
