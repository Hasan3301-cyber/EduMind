package com.example.meetmind.service

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.ListenableWorker.Result
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.example.meetmind.config.ApiConfig
import com.example.meetmind.data.TranscriptQueue
import com.example.meetmind.model.ProcessResponse
import com.example.meetmind.model.RecordingState
import java.io.File
import java.util.concurrent.TimeUnit

class TranscriptSyncWorker(
    appContext: Context,
    workerParameters: WorkerParameters
) : CoroutineWorker(appContext, workerParameters) {
    override suspend fun doWork(): Result {
        val queue = TranscriptQueue(applicationContext)
        val pending = queue.next() ?: return Result.success()
        if (pending.attempts >= MAX_TRANSIENT_ATTEMPTS) {
            val message = "Automatic retry is paused after $MAX_TRANSIENT_ATTEMPTS attempts. Check MeetMind settings, then tap Retry."
            queue.saveResult(ProcessResponse(false, message, createdAt = pending.createdAt))
            RecordingStateBus.publish(RecordingState.Error(message))
            return Result.failure()
        }
        val audioFile = File(pending.audioPath)
        if (!audioFile.isFile) {
            val message = "Queued recording is no longer available on this device."
            queue.remove(pending.id)
            queue.saveResult(ProcessResponse(false, message, createdAt = pending.createdAt))
            RecordingStateBus.publish(RecordingState.Error(message))
            return Result.failure()
        }

        RecordingStateBus.publish(RecordingState.Processing("Transcribing the queued lecture."))
        return try {
            val transcript = AssemblyAiService(ApiConfig.assemblyAiApiKey).transcribe(audioFile)
            val formattedTranscript = transcript.formattedText()
            val syncResult = SupabaseService().syncTranscript(
                transcript = formattedTranscript,
                createdAt = pending.createdAt,
                assemblyTranscriptId = transcript.id
            )
            queue.remove(pending.id)
            audioFile.delete()
            val result = ProcessResponse(
                success = true,
                message = "Transcript synced to ${syncResult.target}.",
                transcript = formattedTranscript,
                syncTarget = syncResult.target,
                remoteId = syncResult.remoteId,
                createdAt = pending.createdAt
            )
            queue.saveResult(result)
            RecordingStateBus.publish(RecordingState.Synced(result.message))
            Result.success()
        } catch (error: ConfigurationException) {
            failWithoutRetry(queue, pending.id, pending.createdAt, error.message)
        } catch (error: PermanentSyncException) {
            failWithoutRetry(queue, pending.id, pending.createdAt, error.message)
        } catch (error: Exception) {
            val attempts = pending.attempts + 1
            val message = if (attempts >= MAX_TRANSIENT_ATTEMPTS) {
                "Automatic retry is paused after $MAX_TRANSIENT_ATTEMPTS attempts. Check MeetMind settings, then tap Retry."
            } else {
                error.message ?: "Transcript sync will retry when connectivity returns."
            }
            queue.recordFailure(pending.id, message)
            queue.saveResult(ProcessResponse(false, message, createdAt = pending.createdAt))
            RecordingStateBus.publish(RecordingState.Error(message))
            if (attempts >= MAX_TRANSIENT_ATTEMPTS) Result.failure() else Result.retry()
        }
    }

    private fun failWithoutRetry(
        queue: TranscriptQueue,
        pendingId: String,
        createdAt: String,
        message: String?
    ): Result {
        val resolvedMessage = message ?: "Transcript sync needs configuration changes."
        queue.recordFailure(pendingId, resolvedMessage)
        queue.saveResult(ProcessResponse(false, resolvedMessage, createdAt = createdAt))
        RecordingStateBus.publish(RecordingState.Error(resolvedMessage))
        return Result.failure()
    }

    companion object {
        fun enqueue(context: Context) {
            val constraints = Constraints.Builder()
                .setRequiredNetworkType(NetworkType.CONNECTED)
                .build()
            val request = OneTimeWorkRequestBuilder<TranscriptSyncWorker>()
                .setConstraints(constraints)
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork(
                UNIQUE_WORK_NAME,
                ExistingWorkPolicy.APPEND_OR_REPLACE,
                request
            )
        }

        private const val UNIQUE_WORK_NAME = "meetmind-transcript-sync"
        private const val MAX_TRANSIENT_ATTEMPTS = 5
    }
}
