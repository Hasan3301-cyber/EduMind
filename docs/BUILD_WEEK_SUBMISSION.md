# EduMind — OpenAI Build Week submission worksheet

> Internal release worksheet. Verify every deadline and requirement against the official Devpost page. Replace every `[PLACEHOLDER]` truthfully before submission. Do not invent Codex use, dates, team information, links, or test evidence.

## Submission fields

### Project name

EduMind

### Category

Education

### Tagline

A local-first AI student operating system that turns a confirmed schedule and trusted course evidence into one student-controlled learning workflow.

### Short description

EduMind unifies planning, class notes, exam practice, spaced repetition, research, routines, and project logs in a private desktop app where evidence stays traceable and important changes require student confirmation.

### Long description

Students often manage classes in a calendar, notes in a document app, revision in a flashcard tool, research in multiple browser tabs, and study questions in a general chatbot. Those systems do not share a trustworthy understanding of the student's real schedule or learning history.

EduMind brings those workflows into one local-first desktop application. Its Planner is the canonical weekly schedule. Routine Coach may suggest study blocks, but nothing enters the schedule until the student reviews and confirms it. Class Notes keeps supplied slides and course material primary while adding bounded transcript evidence. Exam Practice creates clearly labelled practice—not alleged official exam questions—and can turn approved questions into review cards. Study Review previews the scheduling consequence of every grade using the same deterministic backend that persists the review. Research supports focused discovery, selected full-text analysis, source-linked answers, gap inspection, claim validation, supervision, and citation export.

The application is a React/Vite interface packaged with Tauri v2. The Tauri shell starts an authenticated Rust gateway on an OS-assigned loopback port and stores canonical state in versioned local SQLite databases. Agent tools are typed, bounded, allow-listed, and audited without storing content. Provider credentials stay in the operating-system keychain. The core planner, storage, retrieval, SRS, and deterministic research behavior remains usable without an external AI provider.

EduMind's goal is not to automate the student out of learning. It gives the student one coherent operating system for deciding what to study, seeing why an AI suggestion was made, and retaining control over every consequential change.

### The problem

- Student context is fragmented across unrelated applications.
- Generic AI responses are often detached from approved course evidence.
- Scheduling assistants may ignore fixed classes or silently change plans.
- Research tools can hide retrieval gaps or produce unsupported conclusions.
- Cloud-first study tools create avoidable privacy and continuity risks.

### How EduMind solves it

- One canonical Planner feeds the dashboard and read-only routine context.
- Important imports, writes, downloads, and schedule changes require review.
- Notes, research answers, and claims preserve evidence and uncertainty.
- Review previews and persisted schedules share one deterministic algorithm.
- Local SQLite storage, offline paths, bounded tools, and native-keychain secrets keep the student in control.

### Key features to list

1. Confirmed Monday-to-Sunday dashboard schedule.
2. Timetable-image analysis owned exclusively by Student Planner.
3. Source-aware Class Notes with transcript prefetch and safe export.
4. Validated Exam Practice artifacts and optional SRS creation.
5. Deterministic SRS grade previews and durable review history.
6. Focused Research projects, full-text evidence, claim validation, gaps, and BibTeX/RIS export.
7. Routine proposals that cannot silently alter the planner.
8. Student OS cards and project build logs with timestamp/tombstone merge semantics.
9. Typed agent/tool sandbox, bounded execution, and redacted local telemetry.
10. MeetMind Android lecture companion with bounded retry and explicit HTTPS sync destinations.

### How it was built

- **Desktop:** React, Vite, Tauri v2, WebView2
- **Core/runtime:** Rust, Tokio, Axum, Serde
- **Local data:** SQLite, FTS5, persisted embeddings, transactional schema migrations
- **Security:** loopback bearer authentication, native keychain, Argon2id action grants, write sandbox, process limits, deny-by-default tool allow-lists
- **Testing:** Rust unit/integration tests, Clippy, ESLint, Vitest, Node safety tests, Playwright, Android Gradle assembly/lint, real Tauri lifecycle smoke, MSI/NSIS production builds
- **Mobile:** Kotlin/Android, foreground recording service, WorkManager, AssemblyAI, explicit HTTPS EduMind or Supabase sync

### Challenges

- Preserving one canonical schedule while allowing AI to propose—but never silently apply—routine changes.
- Keeping SRS previews exactly consistent with persisted scheduling behavior.
- Combining local lexical/vector retrieval and research provenance without making network access mandatory.
- Packaging an authenticated gateway inside a desktop shell without exposing a fixed local endpoint or credential.
- Supporting useful telemetry without retaining routes, queries, bodies, tokens, transcripts, or student identifiers.
- Keeping mobile transcript retries recoverable while limiting retained audio and transcript data.

### Accomplishments

- Transactional versioned local databases reject unsupported future schemas.
- The installed desktop starts, health-checks, and gracefully stops a real embedded gateway in automated tests.
- The application builds as both MSI and current-user NSIS installers.
- Core learning workflows share canonical typed services instead of duplicating scheduling or persistence logic in the UI.
- Browser workflows exercise planner ownership, dashboard schedule, notes, exam practice, SRS, research, project notes, group study, safety confirmations, and cancellation recovery.

### What is next

- Signed Windows and Android releases.
- A signed update channel and clean-machine upgrade testing.
- Institution-managed deployment and optional encrypted sync without weakening local ownership.
- Larger-scale vector indexing when real corpus sizes justify it.
- Additional accessibility and classroom pilots with student feedback.

### Technologies/tags

Rust, React, Tauri, SQLite, Axum, Tokio, Vite, Playwright, Android, Kotlin, AssemblyAI, local-first, agents, education, spaced repetition, research, privacy

## OpenAI tool-use disclosure

### GPT-5.6

GPT-5.6 Sol was used through Kiro for repository-wide gap analysis, architecture and safety review, migration design, canonical Planner/Routine ownership, workflow completion, telemetry and runtime hardening, release CI, code review, debugging, and validation. Its output was treated as untrusted: proposed behavior was checked against typed contracts and validated with deterministic tests and production builds.

### Codex — required user evidence

Replace this section after completing a genuine task in an official Codex interface:

- **Task performed:** `[ACTUAL_NON_TRIVIAL_CODEX_TASK]`
- **Files or behavior changed/reviewed:** `[ACTUAL_CODEX_SCOPE]`
- **Validation run:** `[ACTUAL_CODEX_VALIDATION]`
- **What the entrant decided or corrected after Codex:** `[HUMAN_DECISION]`
- **Devpost `/feedback` session ID:** enter `[CODEX_FEEDBACK_SESSION_ID]` in the required Devpost field; publish it here only if the official instructions explicitly require that.

Do not describe this Kiro session as an official Codex session.

## Development-period disclosure

Choose and complete the truthful statement:

- `[ ]` EduMind was newly created during the submission period.
- `[ ]` EduMind existed before the submission period and the following meaningful extensions were built during Build Week: `[EXACT_NEW_FEATURES_WITH_EVIDENCE]`.

Supporting evidence: `[REMOTE_GIT_HISTORY_OR_OTHER_TIMESTAMPED_EVIDENCE]`.

## Demo preparation

### Before recording

- Use the latest installed Windows bundle, not the Vite browser preview.
- Use synthetic course names, transcripts, projects, and research questions.
- Hide notifications, email, account names, local paths, keys, and personal data.
- Configure the window near 1440×960 and increase pointer visibility if helpful.
- Preload only the minimum deterministic sample state needed for the story.
- If demonstrating a remote model, configure it before recording and never show the key.
- Record clear English voiceover. Cut loading, typing, and installation time.
- Keep the final video under three minutes; do not rely on content after 3:00.

## Timed demo script (2:55 target)

### 0:00–0:15 — Problem and promise

**Screen:** Launch EduMind and show Home.

**Voiceover:**

> Students manage schedules, notes, revision, research, and AI chats in separate tools that do not share trustworthy context. EduMind is a local-first student operating system that connects those workflows while keeping the student in control.

### 0:15–0:35 — Canonical weekly dashboard

**Screen:** Show the Monday-to-Sunday Final weekly schedule and today’s priorities.

**Voiceover:**

> The dashboard reads only confirmed Planner data. It combines the weekly schedule with local review priorities and never treats an AI draft as a committed event.

### 0:35–0:58 — Timetable review and confirmation

**Screen:** Open Planner, select a synthetic timetable image, analyze it, review confidence/conflicts, and confirm a safe block.

**Voiceover:**

> Timetable images are handled only by Student Planner. EduMind validates the temporary image, presents detected blocks and uncertainty, and requires confirmation before saving canonical state.

### 0:58–1:20 — Class Notes grounded in course evidence

**Screen:** Open Class Notes, show supplied material plus transcript context, generate structured notes, and show source identifiers/export.

**Voiceover:**

> Class Notes keeps the student’s selected resources primary and deterministically retrieves bounded transcript evidence as additional context. Sources and uncertainty remain visible, and exports stay inside the approved output sandbox.

### 1:20–1:40 — Practice and review consequences

**Screen:** Show a validated Exam Practice question, then Study Review with Again/Good/Easy consequences.

**Voiceover:**

> Generated questions are labelled as practice, validated before reuse, and can become review cards only after approval. Every review grade previews its real scheduling consequence using the same backend calculation that will be persisted.

### 1:40–2:05 — Evidence-grounded research

**Screen:** Open a Research project, show selected full-text evidence, validate one supported and one unsupported claim, and show citation export.

**Voiceover:**

> Research downloads only a selected source, links answers to indexed passages, surfaces retrieval gaps, and separates supported claims from unsupported ones before exporting citations.

### 2:05–2:25 — Student control

**Screen:** Show Projects & Notes, then Routine Coach proposing blocks with a confirmation step.

**Voiceover:**

> Student-owned project logs remain local. Routine Coach can read the canonical schedule and propose realistic study blocks, but it cannot alter the plan without explicit approval.

### 2:25–2:45 — Technical implementation

**Screen:** Show a simple architecture slide or README diagram, then return to the app.

**Voiceover:**

> EduMind packages React with Tauri and an embedded Rust gateway. Versioned SQLite stores, typed tool allow-lists, bounded processes, native-keychain credentials, and redacted local telemetry provide a production-oriented foundation without a separately exposed backend.

### 2:45–2:55 — Codex, GPT-5.6, and close

**Voiceover template — replace the Codex placeholder truthfully:**

> I used GPT-5.6 for architecture, workflow implementation, safety review, debugging, and validation. I used Codex to `[ACTUAL_CODEX_CONTRIBUTION]`, then verified the result with `[ACTUAL_TESTS]`. EduMind gives students one evidence-aware learning system without taking away their decisions.

## Latest local release evidence

Validated against current source on July 21, 2026:

- Rust workspace: formatting passed; 198 tests passed; Clippy passed with warnings denied.
- Desktop: ESLint passed; 28 Vitest tests passed; four Node safety tests passed; Vite production build passed.
- Tauri: formatting passed; 15 tests passed including real embedded gateway start/health/stop; Clippy passed with warnings denied.
- Browser: all 17 Playwright workflows passed.
- Android: `assembleDebug` and `lintDebug` passed with the Android Studio JDK.
- Windows: optimized Tauri release produced current x64 MSI and NSIS bundles.

Release checksums:

```text
EduMind_0.1.0_x64_en-US.msi
SHA-256 0CC860C967130AB2C4ACC0E0889BD569E4323F5FEDF0E5F1DE2E58871FBE45D0

EduMind_0.1.0_x64-setup.exe
SHA-256 315D5FDCD6906A6CAC7855173C75AF87A6D6479255571E5495AA240743F6963A
```

Both bundles are intentionally unsigned development artifacts and may trigger Windows SmartScreen. Code signing and a clean-machine install/uninstall check remain release-owner tasks.

## Fresh-install acceptance check

Test the final bundle on a clean Windows user or VM:

- [ ] Installer completes without development tools installed.
- [ ] EduMind launches without a terminal or separate backend.
- [ ] Continue offline opens the local experience.
- [ ] Home displays the full weekly schedule.
- [ ] A Planner block survives restart.
- [ ] A Student OS project note survives restart.
- [ ] Removing a record leaves the rest of the page intact.
- [ ] Invalid project links and invalid timetable files are rejected.
- [ ] No key, transcript, personal path, or raw request appears in logs/telemetry.
- [ ] Uninstall completes; document whether app-data is intentionally retained.
- [ ] Windows SmartScreen behavior for the unsigned build is disclosed.

## Repository-publication check

- [ ] Real Git history and remote are restored or supplied.
- [ ] README placeholders are replaced.
- [ ] No `.env`, `local.properties`, database/WAL, transcript, recording, `OUTPUT`, test-results, Playwright report, `dist`, `target`, Gradle build, keystore, or signing certificate is committed.
- [ ] `LICENSE` is present and ownership wording is accepted by the entrant/team.
- [ ] Source setup works from a fresh clone.
- [ ] Release installers are uploaded as release artifacts, not committed as source.
- [ ] Private repositories grant the official reviewer accounts access before the deadline.

## Devpost final checklist

### Engineering evidence

- [x] Education category selected for the draft.
- [x] Problem, solution, implementation, challenges, impact, and future-work copy prepared.
- [x] Public README quick start and architecture prepared.
- [x] MIT license text added.
- [x] Security/private-data publication exclusions documented.
- [x] Current-source release validation and Windows bundle rebuild completed.
- [ ] Fresh clean-machine installation tested by the entrant.

### User/account actions

- [ ] Verify the official deadline and rules directly on Devpost.
- [ ] Supply `[TEAM_REPRESENTATIVE]` and all `[TEAM_MEMBERS]`; every invitation is accepted.
- [ ] Restore/supply `https://github.com/Hasan3301-cyber/EduMind` and verify reviewer access.
- [ ] Complete a genuine official Codex task and obtain `[CODEX_FEEDBACK_SESSION_ID]` with `/feedback`.
- [ ] Replace every placeholder in README/submission copy.
- [ ] Record, review, and upload the voiceover demo as Public or Unlisted: `[VIDEO_URL]`.
- [ ] Confirm the video explicitly and accurately explains Codex and GPT-5.6 use.
- [ ] Add installer/testing instructions and any required release link: `[RELEASE_URL]`.
- [ ] Submit the Devpost entry.
- [ ] Open **My Projects** and verify the entry is green **Submitted**, not Draft.
- [ ] Save submission confirmation screenshots/email.

## Links to complete

- Repository: `https://github.com/Hasan3301-cyber/EduMind`
- Windows release: `[RELEASE_URL]`
- Demo video: `[VIDEO_URL]`
- Devpost project: `[DEVPOST_PROJECT_URL]`
- Team representative: `[TEAM_REPRESENTATIVE]`
- Team members: `[TEAM_MEMBERS]`
