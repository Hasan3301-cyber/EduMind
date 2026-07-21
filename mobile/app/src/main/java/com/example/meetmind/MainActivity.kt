package com.example.meetmind

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.core.content.ContextCompat
import com.example.meetmind.model.MeetMindScreen
import com.example.meetmind.ui.DashboardScreen
import com.example.meetmind.ui.LoginScreen
import com.example.meetmind.ui.MeetMindViewModel
import com.example.meetmind.ui.ResultScreen
import com.example.meetmind.ui.theme.MeetMindTheme

class MainActivity : ComponentActivity() {
    private val viewModel: MeetMindViewModel by viewModels()
    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { grants ->
        if (grants[Manifest.permission.RECORD_AUDIO] == true) {
            viewModel.startRecording()
        } else {
            viewModel.reportPermissionError()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            val state by viewModel.uiState.collectAsState()
            MeetMindTheme {
                when (state.screen) {
                    MeetMindScreen.Login -> LoginScreen(
                        configurationErrors = state.configurationErrors,
                        onContinue = viewModel::continueToDashboard
                    )
                    MeetMindScreen.Dashboard -> DashboardScreen(
                        recordingState = state.recordingState,
                        queuedCount = state.queuedCount,
                        onStartRecording = ::requestOrStartRecording,
                        onStopRecording = viewModel::stopRecording,
                        onOpenResult = viewModel::showResult,
                        onRetryPending = viewModel::retryPendingSync
                    )
                    MeetMindScreen.Result -> ResultScreen(
                        result = state.lastResult,
                        onBack = viewModel::showDashboard
                    )
                }
            }
        }
    }

    private fun requestOrStartRecording() {
        val permissions = buildList {
            add(Manifest.permission.RECORD_AUDIO)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                add(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
        val missing = permissions.filter { permission ->
            ContextCompat.checkSelfPermission(this, permission) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isEmpty()) {
            viewModel.startRecording()
        } else {
            permissionLauncher.launch(missing.toTypedArray())
        }
    }
}
