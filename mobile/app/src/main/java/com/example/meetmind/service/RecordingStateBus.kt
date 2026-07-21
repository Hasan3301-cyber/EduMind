package com.example.meetmind.service

import com.example.meetmind.model.RecordingState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

object RecordingStateBus {
    private val mutableState = MutableStateFlow<RecordingState>(RecordingState.Idle)
    val state: StateFlow<RecordingState> = mutableState.asStateFlow()

    fun publish(next: RecordingState) {
        mutableState.value = next
    }
}
