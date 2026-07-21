package com.example.meetmind.audio

import android.content.Context
import android.media.MediaRecorder
import java.io.File
import java.time.Instant

class AudioRecorder(private val context: Context) {
    private var recorder: MediaRecorder? = null
    private var outputFile: File? = null

    fun start(): File {
        check(recorder == null) { "A recording is already in progress." }
        val recordingsDirectory = File(context.filesDir, "recordings").apply { mkdirs() }
        val destination = File(recordingsDirectory, "lecture-${Instant.now().toEpochMilli()}.m4a")
        val activeRecorder = MediaRecorder().apply {
            setAudioSource(MediaRecorder.AudioSource.MIC)
            setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
            setOutputFile(destination.absolutePath)
            setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
            setAudioEncodingBitRate(128_000)
            setAudioSamplingRate(44_100)
        }
        try {
            activeRecorder.prepare()
            activeRecorder.start()
            recorder = activeRecorder
            outputFile = destination
            return destination
        } catch (error: Exception) {
            activeRecorder.release()
            destination.delete()
            throw error
        }
    }

    fun stop(): File? {
        val activeRecorder = recorder ?: return null
        val destination = outputFile
        return try {
            activeRecorder.stop()
            destination
        } catch (error: RuntimeException) {
            destination?.delete()
            null
        } finally {
            activeRecorder.release()
            recorder = null
            outputFile = null
        }
    }

    fun discard() {
        val activeRecorder = recorder ?: return
        activeRecorder.reset()
        activeRecorder.release()
        outputFile?.delete()
        recorder = null
        outputFile = null
    }
}
