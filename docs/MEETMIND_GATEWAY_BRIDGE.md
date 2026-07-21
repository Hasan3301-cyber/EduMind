# MeetMind to EduMind Gateway Bridge

## Purpose

The MeetMind Android app can send a completed lecture transcript directly into
the Class Notes module memory. This path is an alternative to the Supabase
meetings-table integration and uses the existing typed gateway memory route.

## Required endpoint

Expose an intentionally provisioned HTTPS service that forwards authenticated
requests to the EduMind gateway:

| Method | Path |
| --- | --- |
| POST | /api/v1/modules/class-notes/memory/store |

The Android client sets EDUMIND_GATEWAY_URL to the HTTPS origin only; it appends
the route above. When EDUMIND_GATEWAY_TOKEN is configured, it adds an
Authorization: Bearer <token> header.

Do not configure a phone with the desktop shell's loopback endpoint. It is
reachable only on the desktop host, uses a per-launch token, and is intentionally
not a network service. Deploy a separate TLS-terminated gateway or trusted
reverse proxy with explicit authentication, rate limits, and request-size limits.

## Request contract

MeetMind sends application/json with this shape:

~~~json
{
  "content": "Speaker A: ...",
  "content_type": "transcript",
  "scope": "module",
  "metadata": {
    "source": "meetmind",
    "created_at": "2026-07-17T10:00:00Z",
    "assemblyai_transcript_id": "..."
  }
}
~~~

The route persists a Class Notes module memory record. The response is expected
to be a JSON object containing record.id when an identifier is available.
MeetMind treats 4xx responses as permanent configuration or authorization
failures and retries unavailable or malformed remote responses through its
WorkManager queue.

## Security and privacy

- Require HTTPS end to end. MeetMind rejects a non-HTTPS direct gateway URL and
  its Android network policy disallows cleartext traffic.
- Authenticate at the edge and at the gateway; scope credentials narrowly to
  the class-notes transcript-store action.
- Do not log raw audio, raw transcripts, bearer credentials, or AssemblyAI
  credentials. Use metadata only for operational tracing.
- Configure body-size, rate, timeout, and retention limits at the reverse proxy.
- Treat audio and transcript content as private educational data. Obtain the
  recorder's consent and follow institutional policy before recording a class.

## Class Notes consumption

When Class Notes runs in Auto mode, it retrieves matching module memories with
module_memory_search using module_id class-notes and content_type transcript.
It must still analyze the lecture slides and supplied resources, preserve source
links, and clearly distinguish transcript evidence from generated explanation.

## Supabase inbox fallback

When MeetMind is configured with SUPABASE_URL and SUPABASE_ANON_KEY instead of
the direct HTTPS bridge, it writes completed transcripts to the meetings table.
The installed EduMind desktop app exposes a MeetMind inbox inside Class Notes:

1. Save the same HTTPS Supabase URL and anonymous key. The URL is stored in
   local settings and the key stays in the operating system keychain.
2. Refresh the inbox to inspect bounded transcript previews and timestamps.
3. Explicitly import a selected transcript into class-notes module memory.
4. Optionally choose to delete the remote record only after the local import
   succeeds and the user confirms the destructive action.

The browser preview does not access the Supabase key. The inbox is not an
automatic deletion or scheduling mechanism; students retain control of every
durable import and external deletion.
