package com.example.meetmind.config

import com.example.meetmind.BuildConfig

object ApiConfig {
    val assemblyAiApiKey: String = BuildConfig.ASSEMBLYAI_API_KEY.trim()
    val supabaseUrl: String = BuildConfig.SUPABASE_URL.trim().trimEnd('/')
    val supabaseAnonKey: String = BuildConfig.SUPABASE_ANON_KEY.trim()
    val eduMindGatewayUrl: String = BuildConfig.EDUMIND_GATEWAY_URL.trim().trimEnd('/')
    val eduMindGatewayToken: String = BuildConfig.EDUMIND_GATEWAY_TOKEN.trim()

    val usesDirectGateway: Boolean
        get() = eduMindGatewayUrl.isNotBlank()

    fun validationErrors(): List<String> = buildList {
        if (assemblyAiApiKey.isBlank()) {
            add("ASSEMBLYAI_API_KEY is missing.")
        }
        if (usesDirectGateway) {
            if (!eduMindGatewayUrl.startsWith("https://")) {
                add("EDUMIND_GATEWAY_URL must use HTTPS.")
            }
        } else {
            if (supabaseUrl.isBlank() || supabaseAnonKey.isBlank()) {
                add("Configure a direct EduMind gateway or both Supabase settings.")
            } else if (!supabaseUrl.startsWith("https://")) {
                add("SUPABASE_URL must use HTTPS.")
            }
        }
    }
}
