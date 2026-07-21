# MeetMind Android Companion

MeetMind is the Android lecture companion for EduMind. It records lecture audio
through a foreground microphone service, uploads it to AssemblyAI for
speaker-labelled transcription, and synchronizes the completed transcript to
either EduMind Class Notes memory or a Supabase meetings table.

## Prerequisites

- Android Studio with a JDK capable of compiling Java 17 sources.
- Android SDK Platform 34 and matching build tools.
- A physical device or emulator running Android 8.0 (API 26) or newer.
- An AssemblyAI API credential. Production deployments should use a restricted
  credential or a trusted transcription proxy; Android BuildConfig injection
  prevents source commits, but it is not a secret vault for a shipped APK.

## Configure a local build

From the mobile directory, copy the non-secret template:

~~~powershell
Copy-Item local.properties.example local.properties
~~~

Set ASSEMBLYAI_API_KEY and choose exactly one transcript destination:

- Direct EduMind bridge: set EDUMIND_GATEWAY_URL to a deliberate HTTPS gateway
  or reverse proxy. Set EDUMIND_GATEWAY_TOKEN when that gateway requires bearer
  authentication.
- Supabase: leave EDUMIND_GATEWAY_URL blank and set both SUPABASE_URL and
  SUPABASE_ANON_KEY. The app posts transcript and created_at records to the
  meetings table. In the installed EduMind desktop app, open Class Notes and
  save the same Supabase URL and key in the MeetMind inbox. Review each
  transcript before importing it into local Class Notes memory; remote deletion
  is optional and requires a separate confirmation.

The same five setting names may come from Gradle properties or environment
variables in CI. Never commit local.properties. The app rejects a direct
gateway URL that is not HTTPS and Android network security disables cleartext
traffic.

## Build and install

~~~powershell
$env:JAVA_HOME = "C:\Program Files\Android\Android Studio\jbr"
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:ANDROID_SDK_ROOT = $env:ANDROID_HOME
.\gradlew.bat :app:assembleDebug
~~~

The debug APK is written to app/build/outputs/apk/debug/app-debug.apk. Android
asks for microphone permission before recording and notification permission on
Android 13 or later.

## Sync lifecycle

1. RecordingService starts as a microphone foreground service and writes a
   temporary M4A file in app-private storage.
2. Stopping a recording puts its file in a persistent, on-device queue.
3. WorkManager uploads the file, creates an AssemblyAI universal-2 transcript
   for English and Bengali, polls until completion, and preserves speaker
   labels.
4. The worker sends the formatted transcript to the configured destination.
   Transient failures retry with a bounded backoff; permanent failures remain
   visible in the dashboard so the user can retry after fixing configuration.
   Automatic retries stop after five attempts; tapping Retry resets that limit.
5. Successful records remove local audio and queue metadata.

MeetMind enforces a 100 MiB audio upload limit and bounds remote response sizes.
It does not connect to the desktop app's ephemeral loopback gateway; use the
HTTPS bridge described in ../docs/MEETMIND_GATEWAY_BRIDGE.md.
