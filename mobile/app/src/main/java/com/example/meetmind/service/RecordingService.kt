package com.example.meetmind.service

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import com.example.meetmind.audio.AudioRecorder
import com.example.meetmind.data.TranscriptQueue
import com.example.meetmind.model.RecordingState
import java.time.Instant

class RecordingService : Service() {
    private val audioRecorder by lazy { AudioRecorder(this) }
    private var recording = false

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> startRecording()
            ACTION_STOP -> stopRecording()
        }
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        if (recording) {
            audioRecorder.discard()
            RecordingStateBus.publish(RecordingState.Error("Recording stopped before the lecture audio could be saved."))
        }
        super.onDestroy()
    }

    private fun startRecording() {
        if (recording) {
            return
        }
        startMicrophoneForeground(NOTIFICATION_ID, notification("Recording lecture audio"))
        try {
            audioRecorder.start()
            recording = true
            RecordingStateBus.publish(RecordingState.Recording(Instant.now().toString()))
        } catch (error: Exception) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            RecordingStateBus.publish(RecordingState.Error(error.message ?: "Microphone recording could not start."))
            stopSelf()
        }
    }

    private fun stopRecording() {
        if (!recording) {
            stopSelf()
            return
        }
        val audioFile = audioRecorder.stop()
        recording = false
        if (audioFile == null) {
            RecordingStateBus.publish(RecordingState.Error("The recording was too short or could not be saved."))
        } else {
            TranscriptQueue(applicationContext).enqueue(audioFile)
            TranscriptSyncWorker.enqueue(applicationContext)
            RecordingStateBus.publish(RecordingState.Queued("Lecture audio is queued for transcription and sync."))
        }
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun startMicrophoneForeground(notificationId: Int, notification: android.app.Notification) {
        createNotificationChannel()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(notificationId, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
        } else {
            startForeground(notificationId, notification)
        }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return
        }
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Lecture recording",
            NotificationManager.IMPORTANCE_LOW
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun notification(content: String): android.app.Notification = NotificationCompat.Builder(this, CHANNEL_ID)
        .setSmallIcon(android.R.drawable.ic_btn_speak_now)
        .setContentTitle("MeetMind")
        .setContentText(content)
        .setOngoing(true)
        .build()

    companion object {
        fun startIntent(context: Context): Intent = Intent(context, RecordingService::class.java).setAction(ACTION_START)

        fun stopIntent(context: Context): Intent = Intent(context, RecordingService::class.java).setAction(ACTION_STOP)

        private const val ACTION_START = "com.example.meetmind.action.START_RECORDING"
        private const val ACTION_STOP = "com.example.meetmind.action.STOP_RECORDING"
        private const val CHANNEL_ID = "lecture-recording"
        private const val NOTIFICATION_ID = 4101
    }
}
