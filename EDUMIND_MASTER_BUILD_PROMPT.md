# EduMind — Master Build Prompt

> **Purpose of this document.** This is a complete, self-contained build
> specification you can hand to an autonomous coding agent (Claude Code, Codex,
> etc.) to build **EduMind** — a local-first, multi-agent AI study assistant for
> students — from scratch, or to produce an upgraded next version. It captures
> the architecture, every subsystem, the data contracts, the security model,
> the desktop app, testing/CI expectations, coding standards, and a phased
> build plan with acceptance criteria.
>
> Read this whole document before writing code. Build in the phases given.
> After each phase, the workspace must compile, pass tests, and be lint-clean.

---

## 0. How to use this prompt (instructions to the building agent)

- Treat this as the product + engineering spec. Where a detail is unspecified,
  choose the simplest robust option that fits the stated architecture and note
  the decision.
- **Work in phases** (Section 18). Do not jump ahead. Each phase ends with a
  green build (`cargo build`), green tests (`cargo test --workspace`), and a
  clean linter (`cargo clippy --workspace --all-targets -- -D warnings`).
- Prefer **pure, unit-testable modules** with narrow public APIs. Every
  non-trivial module ships with `#[cfg(test)]` tests.
- **Determinism first.** Analysis features (ranking, gaps, synthesis) must be
  deterministic and dependency-light so they work offline and in CI. LLM/network
  calls are optional enhancements layered on top, never required for tests.
- **Security is not optional.** Implement the security model in Section 14 as
  you build the surfaces it protects, not as an afterthought.
- Cross-platform, but the primary dev/target OS is **Windows**; all external
  process calls, path handling, and sandboxing must work on Windows.
- Do **not** hardcode absolute machine paths. Use config + env + workspace-
  relative resolution.

---

## 0.1 Mandatory upgrades for THIS version (must-have, not optional)

This build is an **upgraded** EduMind. The following three changes are hard
requirements and take priority over parity with the previous version. Every
section below has been written to reflect them; if any older phrasing conflicts,
these win.

1. **Single installable desktop app — no separately-run backend.** The user
   installs one app (Windows `.msi`/`.exe`, and ideally macOS `.dmg` / Linux
   `AppImage`) and everything runs inside it. The gateway must **not** require
   the user to launch a separate server process. Preferred approach: link the
   `edumind` gateway **as a library directly into the Tauri Rust process** and
   start it in-process on app launch (bind to `127.0.0.1` on an ephemeral/free
   port, auth on, loopback-only). A bundled auto-managed sidecar is an
   acceptable fallback only if in-process embedding is impractical — but there
   must be **zero** manual backend steps for the end user. See Section 15.

2. **All graphs are interactive 3D.** Every graph the app shows — knowledge
   graph, literature/citation graph, memory graph, community/cluster views — is
   rendered as an **interactive 3D force-directed graph** (WebGL/Three.js), not
   a 2D diagram. See Section 15B.

3. **More efficient memory management.** Upgrade the memory subsystem for speed
   and scale: an approximate-nearest-neighbor (ANN) vector index, quantized/
   compact embeddings, tiered hot/warm/cold storage with summarization +
   eviction, incremental indexing, and bounded caches. Must stay local-first and
   deterministic in tests. See Section 8.

---

## 1. Product overview

**EduMind** is a local-first AI operating system for students. A single Rust
**gateway** process hosts a fleet of configurable agents and sub-agents, a
persistent memory system, and a set of study-focused capability "modules." A
**Tauri + React desktop app** is the primary UI and can launch the gateway as a
bundled sidecar. Everything works offline with local models (Ollama) and
degrades gracefully; cloud model providers are optional.

### Design tenets
1. **Local-first, private & self-contained.** SQLite storage, local embeddings
   by default, no mandatory cloud dependency — and **no separate backend to
   run**: the gateway is embedded in the installed desktop app (Section 15).
2. **Multi-agent orchestration.** A master agent routes work to specialized
   module sub-agents, which can spawn their own sub-sub-agents.
3. **Persistent, structured memory.** Full-text + vector + knowledge graph +
   a background "learning loop."
4. **Deterministic cores, optional intelligence.** Core logic is testable and
   offline; models enrich but are never required.
5. **Configuration-driven.** Behavior, routing, agents, and providers come from
   YAML config with hot-reload.

### The six student modules (product surface)
1. **Class Notes** — turn a lecture (DB-synced transcript, optional) + teacher
   slides + topper notes + past questions into exam-focused notes (LaTeX→PDF).
2. **CT Prep** — combine syllabus + class notes + topper style into exam-ready
   prep material.
3. **Lab Report** — generate formal reports from a LaTeX template + images.
4. **Routine** — merge the Planner's 7-day class schedule with personal
   commitments (gym, Telegram, Gmail, CSV) into a daily routine.
5. **Research** — an ongoing research *assistant/supervisor* (persistent
   projects, full-text PDF reading, gap analysis, supervision, bibliography).
6. **Mission & Vision** — progress tracking, tests, roadmap, timeline.

### Companion surfaces (first-class, not optional)
- **Student OS** — a personal life/study dashboard (routines, workout/gym plan,
  habits, personal state) rendered as editable cards in the desktop app and
  persisted server-side so agents can read/update it (e.g. Module 4 pulls the
  gym schedule from here).
- **Student Planner** — the weekly study workspace that turns the uploaded
  **class-routine image into the canonical 7-day class schedule** and study
  plan. Module 4 (Routine) reads this schedule; the routine image is uploaded
  **here only** (never re-OCR'd elsewhere).
- **MeetMind mobile app** — an Android companion that records lectures,
  transcribes them, and syncs the transcript so **Class Notes (Module 1)** can
  fetch it from the database in Auto mode. See Section 15A.

Student OS and Planner state is the shared source of truth between the UI, the
agents (via tools/endpoints), and the mobile transcript feed — treat all three
as core, not add-ons.

---

## 2. Repository / workspace layout

Produce a Cargo workspace plus a standalone desktop app:

```
/                                  # Cargo workspace root
  Cargo.toml                       # [workspace] members + release profile
  crates/
    edumind-core/                  # shared domain types (no heavy deps)
      src/{lib,domain,research,runs}.rs
  edumind/                         # the gateway binary + library
    Cargo.toml
    config.example.yaml            # documented default config
    routing.yaml                   # module routing rules
    src/
      main.rs  lib.rs
      gateway/  agent/  memory/  research/  routing/ channels/
      config/   plugins/ runtime_tools/ study/ web/ mcp/ infra/
      security.rs  secrets.rs  student.rs  runs.rs
    plugins/                       # on-disk plugin manifests (research/*)
  edumind-tauri/                   # desktop app (standalone build)
    package.json  vite.config.js  index.html  playwright.config.js
    src/                           # React 18 + Vite frontend
    src-tauri/                     # Tauri (Rust) shell + sidecar wiring
  mobile/                          # MeetMind Android companion (Kotlin/Gradle)
    app/src/main/java/com/example/meetmind/
      audio/  config/  model/  service/  ui/  MainActivity.kt
  Sandbox/AGENTS.md                # agent registry & module operating rules
```

Workspace `Cargo.toml`:
- `resolver = "2"`, members = `crates/edumind-core`, `edumind`.
- `exclude` the Tauri `src-tauri` (builds via Tauri CLI with its own lockfile).
- `[profile.release]`: `lto = true`, `codegen-units = 1`, `strip = true`.

---

## 3. Tech stack & dependencies (pin these)

### `edumind-core` (pure domain types — keep dependency-light)
`serde`(derive), `serde_json`, `chrono`(serde), `uuid`(v4), `async-trait`.

### `edumind` gateway
- Runtime/web: `tokio`(full), `axum` 0.8 (`ws`,`macros`), `tower`, `tower-http`
  (`cors`,`trace`).
- Serde: `serde`, `serde_json`, `serde_yaml`.
- DB: `rusqlite` 0.32 (`bundled` — ships SQLite, FTS5 available).
- HTTP client: `reqwest` 0.12 (`json`,`stream`,`rustls-tls`,`blocking`, no
  default features).
- Streaming: `futures`, `tokio-stream`, `async-stream`.
- Logging: `tracing`, `tracing-subscriber`(`env-filter`,`json`).
- Config watch: `notify` 7.
- Utility: `uuid`(v4), `chrono`(serde), `thiserror`, `anyhow`, `dashmap`,
  `parking_lot`, `regex`, `glob`, `walkdir`, `bytes`, `base64`, `hex`.
- Crypto/auth: `sha2`, `hmac`, `rand`, `argon2` (action-password hashing),
  `jsonwebtoken` 9 (JWT), `keyring` 3 (`windows-native`) for OS secret storage.
- Channels: `teloxide` 0.13 (`rustls`, no defaults) for Telegram.
- CLI: `clap` 4 (derive).
- PDF: `pdf-extract` 0.7 (text-layer extraction).
- XML: `quick-xml` 0.37 (arXiv Atom).
- Vector index (UPGRADE): a pure-Rust ANN/HNSW crate (e.g. `hnsw_rs` or
  `instant-distance`) for the efficient memory index; keep an exact fallback.
- Windows: `windows-sys` 0.59 (`Win32_Foundation`, `Win32_Security`,
  `Win32_System_JobObjects`, `Win32_System_Threading`) for process job objects.

### Desktop app (`edumind-tauri`)
React 18 + Vite (rolldown/vite), `@tauri-apps` v2, ESLint (with
`eslint-plugin-react`, `eslint-plugin-unused-imports`), Playwright for e2e,
bundled fonts (`@fontsource`) and `@fortawesome` icons (offline-capable).
**3D graphs:** `react-force-graph-3d` + `three` (+ `three-forcegraph`), bundled
locally. The Tauri Rust crate (`src-tauri`) depends on the `edumind` **library
crate** so the gateway runs in-process (embedded backend).

### Mobile companion (`mobile/` — MeetMind, Android)
Kotlin + Jetpack Compose (Material 3), Gradle Kotlin DSL, OkHttp + coroutines
for networking, AssemblyAI for transcription, Supabase (or the EduMind gateway)
for transcript sync. Secrets injected via `BuildConfig` (never hardcoded).

External runtime tools (optional, detected at runtime — **not** build deps):
- **Ollama** (local models + embeddings, e.g. `nomic-embed-text`).
- **ocrmypdf** + Tesseract + Ghostscript (OCR for scanned PDFs).
- A LaTeX toolchain / compile service (for notes/report PDF output).
- **Node.js** (slide→pptx, some doc/slide scripts), Python (helper scripts,
  NotebookLM bridge).

---

## 4. `edumind-core` — shared domain types

A dependency-light crate that is the single source of truth for domain
entities. Modules:

- `domain.rs` — core task/entity types shared across the system.
- `runs.rs` — pipeline run records (`PipelineRun`, stage records, status).
- `research.rs` — the research domain model (see Section 9). Includes:
  - Plugin contract types: `PluginPriority`, `PluginStage` (start/end range,
    `single`/`range`/`continuous`, `contains`, `from_label`), `PluginStatus`,
    `PluginMetadata`, `PluginOutput`, `PluginInfo`, and the `ResearchPlugin`
    async trait (`name`, `priority`, `stage`, `description`, `initialize`,
    `execute`, `should_run`, `dependencies`, `as_any`).
  - Data types: `PaperMetadata` (id, title, authors, abstract_text, year,
    venue, doi, citation_count, open_access_url, source_url, source, per-source
    ids, keywords, fields_of_study, referenced/influenced ids, fetched_at),
    `StructuredDocument`, `ResearchInsight`/`InsightType`, `Hypothesis`,
    `GraphData`/`GraphNode`/`GraphEdge`/`Community`, `ValidationReport` and its
    parts, `PipelineContext` (accumulator passed through plugin stages),
    `PipelineProgress`, `PipelineEvent`.
  - Persistent workspace types: `ResearchProject` (id, topic, questions, scope,
    papers, notes, last_run_id, timestamps) with `add_papers` (dedup by DOI →
    arXiv id → normalized title), `add_note`, `add_question`; and `ProjectNote`.

All types `Serialize`/`Deserialize`; use `#[serde(default)]` on new/optional
fields so persisted JSON stays forward/backward compatible.

---

## 5. The gateway — module map

`edumind/src/lib.rs` exposes these top-level modules; `main.rs` is a thin
`clap` CLI that loads config and starts the server.

- `gateway/` — HTTP + WebSocket server, protocol, auth, routing to handlers,
  broadcast, chat run-state, collab, scheduler, `AppState`.
- `agent/` — model resolution/failover, agent + sub-agent registries, session
  manager, prompt building, tool definitions + tool policy, skills, compaction,
  identity, model telemetry, sandbox.
- `memory/` — SQLite store, embeddings, vector store, hybrid search, knowledge
  graph, LM-wiki, Hermes learning loop, module memory, memory pipeline,
  advanced search, intelligence, EduMind bridge.
- `research/` — pipeline engine + plugins, persistent projects, full-text
  ingest/RAG, OCR, supervisor, synthesis, bibliography, claim validation.
- `routing/` — `routing.yaml` module router, route resolver, session keys.
- `channels/` — desktop + Telegram channels behind a manager.
- `config/` — typed config, loader/validation, hot-reload watcher, redaction.
- `plugins/` — plugin registry/loader/runtime, execution sandbox, hooks.
- `runtime_tools/` — LaTeX compile, slide engine, doc engine, image tools,
  **process guard** (sandboxed external command runner).
- `study/` — spaced-repetition (SRS) service.
- `web/` — SearXNG + Tavily web search, academic literature connectors.
- `mcp/` — Model Context Protocol integration (NotebookLM, node + python
  bridges, path helpers).
- `infra/` — error types (`EduMindError`, `Result`), blocking-bridge helper.
- top-level: `security.rs`, `secrets.rs` (OS keychain via `keyring`),
  `student.rs` (Student OS/Planner state store), `runs.rs` (run store).

---

## 6. Gateway server, protocol & endpoints

### Server (`gateway/server.rs`)
- `AppState` is an `Arc`-shared struct holding every subsystem handle: config
  (`Arc<RwLock<EduMindConfig>>`), auth, memory store, run store, project store,
  **fulltext store**, student pages, SRS, embedder (`Arc<RwLock<TextEmbedder>>`),
  vector store, search engine, module memory, session manager, sub-agent
  registry, chat run-state, collab service, broadcaster, plugin registry +
  runtime, hook runner, tool audit log, knowledge graph, LM-wiki, Hermes
  (`Arc<RwLock<..>>`), channel manager, event sender, module router, config path.
- Databases live next to `memory.db_path`: `memory.db`, `runs.db`,
  `research_projects.db`, `research_fulltext.db`.
- Startup: build router, bind TCP listener, spawn background tasks:
  config hot-reload watcher, gateway event fan-out, channel event processor,
  Telegram polling (if enabled), scheduler (recurring jobs), Hermes loop.
- **Security guard:** refuse to start when `auth.mode == none` **and** the bind
  address is non-loopback, unless `EDUMIND_ALLOW_INSECURE_BIND=1`. Implement a
  `bind_is_loopback` helper (handles `localhost`, IPv4/IPv6 loopback, brackets).
- Config apply: distinguish **reloadable** vs **restart-required** changes
  (port/bind/mode/remote/body-limit/channels/memory/session/jobs require
  restart); apply reloadable changes live (reload auth, plugins, router).

### Frame protocol (`gateway/protocol.rs`)
- `RequestFrame { id, method, params }`, `ResponseFrame { id, ok, payload?,
  error? }` with `ok`/`error` constructors, `EventFrame { event, payload }`,
  `ProtocolError { code, message }`, `ConnectParams`, `PROTOCOL_VERSION`.
- Same frame shape is used over both HTTP (POST body) and WebSocket.

### HTTP + WS (`gateway/http.rs`, `gateway/ws.rs`)
- `build_router(state)` composes routes, then layers: body limit
  (`request_body_max_bytes`), auth middleware, `TraceLayer`, permissive CORS.
- **Auth middleware:** enforce `security.allowed_origins` on the `Origin`
  header; allow `/health`, `/ready`, `/ws` unauthenticated; otherwise require a
  `Bearer` token (token/JWT modes) and inject an `AuthPrincipal` (roles).
- **Endpoint catalog** (all under `/api/v1` unless noted):
  - Health: `GET /health`, `GET /ready`.
  - Chat/agents: `POST /chat`, `POST /agent`, `GET /sessions`.
  - Config (admin role + action password): `GET /config`, `POST /config`.
  - Channels: `GET /channels/status`.
  - Memory: `POST /memory/{search,list,get,store,update,delete,dedupe,
    backfill-embeddings,resync-vectors,maintenance}`.
  - Module memory: `POST /modules/{module_id}/memory/{search,store,get,summary}`.
  - Study/SRS: `POST /study/srs/{card,generate,due,review,stats}`.
  - Research (stateless): `POST /research/{validate-claims,literature-graph,
    focused/run,pipeline/plugins}`.
  - Runs: `GET /runs`, `GET /runs/{run_id}`.
  - **Research projects (assistant):**
    - `GET  /research/projects`, `GET /research/projects/{id}`
    - `POST /research/projects/{id}/ask` (abstract-level grounded Q&A)
    - `POST /research/projects/{id}/synthesis` (comparison matrix + outline)
    - `POST /research/projects/{id}/export` (BibTeX/RIS)
    - `POST /research/projects/{id}/{note,question,scope}` (curation)
    - `POST /research/projects/{id}/ingest` (download/read PDFs → OCR-aware
      extract → chunk → embed → index)
    - `GET  /research/projects/{id}/documents` (list ingested full texts)
    - `POST /research/projects/{id}/deep-ask` (full-text RAG w/ passage citations)
    - `POST /research/projects/{id}/gaps` (author-stated gaps from bodies)
    - `POST /research/projects/{id}/supervise` (supervisor report)
  - Student pages: `POST /student/page-state/{get,save}`.
  - Web: `POST /web/search`.
  - Plugins/skills: `GET /plugins`, `POST /plugins/toggle`, `GET /skills`.
  - Hermes: `GET /hermes/cycles`.
  - Security: `GET/POST /security/action-password`, `GET
    /security/action-password/status`, `POST /security/tool-audit`.
  - Models: `POST /models/telemetry`.
  - Collab: `POST /collab/session/{create,join,get,list}`, `POST
    /collab/{event,state}`.
  - WebSocket: `GET /ws` (connect handshake, streaming chat, event broadcast).

### Broadcast & events
- A `Broadcaster` fans `EventFrame`s to all connected WS clients (config
  reloaded, channel messages/typing/reactions, chat streaming deltas, etc.).

---

## 7. Config subsystem

- `config/types.rs` defines `EduMindConfig` and all nested structs. Every field
  uses `#[serde(default)]` (or a `default_*` fn) and, where the desktop app uses
  camelCase, add serde `alias`. Sections: `meta`, `gateway` (+ `auth`,
  `remote`), `models` (providers→models with cost/context), `agents`
  (`defaults` + `list`), `channels` (desktop, telegram), `memory` (`db_path`,
  `embedding`, `vector`, `hermes`), `plugins`, `tools` (`profile`,
  `latex_compile`, `notebooklm`, `notebooklm_py`, **`ocr`**), `messages`,
  `session`, `web` (searxng, tavily, literature→per-source), `jobs`, `security`.
- `config/loader.rs` — load YAML, expand `${ENV}` and `~`, `validate()`.
- `config/hot_reload.rs` — `notify`-based watcher emitting reload events; the
  server recomputes restart-required changes and applies the rest live.
- `config/redact.rs` — redact secrets (tokens, keys) from any serialized config
  returned over the API.
- Ship a fully-commented `config.example.yaml` mirroring every field with safe
  defaults (auth `none` on loopback for dev, providers for Ollama/OpenRouter/
  Copilot, memory with Ollama embeddings + hash fallback, etc.).

---

## 8. Memory subsystem

- `memory/sqlite_store.rs` — `SqliteMemoryStore` over `rusqlite` (bundled).
  Tables for memories (with FTS5 virtual table), sessions, skills + usage,
  memory events + versions, system key/values, Hermes cycles. Provides store/
  search_fts/get/get_all/update/delete, embedding update, dedupe, session
  load/store/list, skill load/store/usage, stats. Path helper
  `shellexpand_path` (`~` and `${ENV}`).
- `memory/embedding.rs` — `TextEmbedder` with providers: `hash` (deterministic
  local, offline default/fallback), `ollama` (`/api/embeddings`), and
  `openai-compatible` (`/v1/embeddings`, bearer). Normalizes vectors, validates
  dimensions, falls back to hash on failure. Runs network calls off the async
  runtime via a blocking bridge. Expose `embed_text`, `dimensions`,
  `provider_name`, `signature`, `hash_only`.
- `memory/local_embedding.rs` — deterministic hash embedding.
- `memory/vector_store.rs` — `VectorStore` (SQLite-backed vectors; pluggable
  backend name) + `cosine_similarity`.
- `memory/search.rs` — `SearchEngine` combining FTS + vector into
  `search_hybrid` (fts, vector, hybrid with threshold).
- `memory/knowledge_graph.rs` + `knowledge_extractor.rs` — entity/relation graph
  with community detection (Leiden-style); extraction from content.
- `memory/lm_wiki.rs` — auto-generated "wiki" pages from concepts/skills.
- `memory/module_memory.rs` — `ModuleMemoryService`: per-module namespaced
  memory (scopes: private/module/cross-module/global) with its own embeddings;
  search/store/get/summary.
- `memory/hermes.rs` — `HermesLearningLoop`: background loop that refines skill
  confidence from observed success/failure and generates skill insights; gated
  by cooldown; configurable.
- `memory/advanced_search.rs`, `memory_intelligence.rs`, `pipeline.rs`,
  `edumind_bridge.rs` — multi-stage rerank (MMR + jaccard), higher-level
  intelligence, ingestion pipeline, and an import/export bridge.

### 8.1 Efficient memory management (UPGRADE — required)

Replace the brute-force "load-all-vectors, cosine-scan" pattern with a scalable,
still-local, still-deterministic design. Build it in `memory/vector_index.rs`
(new) plus additions to the store; keep the public search API stable.

- **ANN vector index.** Add an approximate-nearest-neighbor index (HNSW) over
  embeddings so search is sub-linear instead of O(N) per query. Prefer a pure-
  Rust HNSW crate (e.g. `hnsw_rs`/`instant-distance`); persist the index to disk
  and rebuild lazily/incrementally. Expose `search_ann(query_vec, k,
  ef_search)`. Keep an **exact fallback** for tiny corpora and for tests
  (deterministic ordering).
- **Compact embeddings.** Store embeddings quantized (e.g. int8 scalar
  quantization, or optional binary/PQ for large stores) with the raw f32 kept
  only where needed. Record the quantization scheme in the row so re-embeds and
  migrations are safe. Cosine works on de-quantized vectors; re-rank the top
  ANN candidates with exact similarity for accuracy.
- **Tiered hot/warm/cold storage.** Keep frequently/recently used memories
  "hot" (fully indexed, fast path); demote stale entries to "warm" (indexed but
  compacted) and "cold" (summarized + archived). A background maintenance task
  (extend the Hermes loop or a dedicated janitor) promotes/demotes by
  recency+frequency+importance and **summarizes cold clusters** into a single
  distilled memory (with links back to sources) to cap growth.
- **Dedup + eviction.** Reuse/extend the existing exact-dedup; add near-dup
  detection (embedding cosine ≥ threshold) at ingest, and a bounded-size policy
  with LRU/importance-weighted eviction to cold tier. Never hard-delete without
  a tombstone + event.
- **Incremental & batched indexing.** Index on write (append to ANN + FTS)
  rather than full rebuilds; batch embedding calls; debounce re-index. Provide
  `maintenance`/`resync` operations that reconcile ANN ↔ SQLite ↔ FTS.
- **Bounded caches.** Add LRU caches (embeddings, hot query results, graph
  neighborhoods) with explicit capacity from config; measure hit rate.
- **Observability & config.** New `memory.index` config block: `backend`
  (`exact` | `hnsw`), `ef_construction`, `ef_search`, `m`, `quantization`
  (`none`|`int8`|`binary`), tier thresholds, cache sizes. Report index stats
  (size, tier counts, recall estimate, cache hit rate) via a memory-stats
  endpoint and the desktop memory panel.
- **Tests.** Deterministic: ANN top-k must match exact top-k on small fixtures
  (with generous `ef`); quantize→dequantize round-trip within tolerance;
  tier promotion/demotion and eviction are deterministic given fixed
  recency/importance inputs. No network in tests (hash embeddings).

---

## 9. Research module (the flagship) — full spec

The research module must behave like an **ongoing research assistant &
supervisor**, not a one-shot report generator. Everything a topic accumulates
lives in a persistent `ResearchProject`.

### 9.1 Pipeline engine + plugins (`research/pipeline.rs`, `research/plugins.rs`)
- `ResearchPipelineEngine` runs a staged pipeline over a `PipelineContext`.
  A typed plugin registry (`TypedResearchPluginRegistry`) holds native plugins:
  - `orchestrator-plugin` (coordination/manifest normalization),
  - `literature-discovery-plugin` (Semantic Scholar / PubMed / arXiv / Scopus,
    or web fallback),
  - `paper-ranking-plugin` (citations + recency + abstract richness),
  - `knowledge-graph-plugin` (literature graph from ranked papers),
  - `insight-generator-plugin` (**real, deterministic** analysis — see below),
  - `hypothesis-engine-plugin` (grounded, testable hypothesis),
  - `critic-plugin` (validate draft claims vs evidence).
  Plus a `LegacyPythonPluginBridge` that runs on-disk Python plugins declared by
  manifest (safe script selection, arg/env parsing, execution limits).
- **Deterministic analysis helpers** (crate-visible, unit-tested):
  `corpus_keyword_frequencies`, `recency_share`, `detect_gaps`,
  `corpus_novelty`, `truncate_chars`. Insights/hypotheses are computed from the
  corpus (dominant themes, recent-share → emerging vs maturing, under-covered
  query terms → candidate gaps), never templated.
- The pipeline records a `PipelineRun` into the run store and merges discovered
  papers into the topic's project (dedup) via the project store.

### 9.2 Persistent projects (`research/project.rs`)
- `ProjectStore` (SQLite `research_projects.db`): `save`, `get`, `find_by_topic`
  (case-insensitive, most-recent), `list_recent`, `get_or_create`, `record_run`
  (merge papers + stamp run id). Repeated runs on the same topic **accumulate**
  into one project (dedup by DOI/arXiv/title).
- Ranking helpers: `rank_papers_for_query` (lexical: title 3×, keyword 1.5×,
  abstract 1×, citation tie-break) and `semantic_rank_papers` (lexical-narrow →
  embedding rerank via cosine on title+abstract).

### 9.3 Full-text "deep reading" (`research/fulltext.rs`)
- `chunk_text(text, target, overlap)` — char-boundary-safe overlapping chunks
  (defaults ~1200/200).
- `extract_pdf_text(bytes)` — text-layer extraction via `pdf-extract` (size
  capped, e.g. 25 MiB); call off the async runtime.
- `FullTextStore` (SQLite `research_fulltext.db`): `fulltext_docs` (per project+
  paper: title, source, char/chunk counts, full_text, extracted_at) and
  `fulltext_chunks` (project, paper, chunk_index, text, embedding BLOB — f32
  LE bytes). APIs: `has_document`, `store_document`, `list_documents`,
  `all_chunks`, `full_texts`.
- `embed_chunks(embedder, text)` and `search_passages(store, embedder, project,
  query, limit)` — RAG over chunk embeddings returning passages with
  `paper_id`, `chunk_index`, `score`.
- `downloadable_url(paper)` — HTTPS open-access/source URL for auto-download.

### 9.4 OCR for scanned PDFs (`research/ocr.rs`)
- `ocr_pdf(config, pdf_path)` — shells out to `ocrmypdf --force-ocr --sidecar
  <txt> -l <lang> -q <in.pdf> <out.pdf>` through the sandboxed process guard
  (Section 13); reads the sidecar text; always cleans up temp files; degrades
  gracefully with a clear error if the tool is missing. **No native OCR
  linking.**
- Config `tools.ocr`: `enabled`, `command`(`ocrmypdf`), `language`(`eng`),
  `timeout_secs`(300), `min_text_chars`(200).
- Ingest is OCR-aware: extract text layer first; if cleaned length <
  `min_text_chars` (or `ocr: "force"`), stage the PDF to a temp file and OCR;
  keep whichever text is longer; report `"ocr": true/false` per paper. Request
  `ocr` mode: `auto` (default) | `force` | `off`.

### 9.5 Supervisor (`research/supervisor.rs`)
- `extract_stated_gaps(docs)` — mine author-stated sentences from full text,
  classified as `limitation` / `future_work` / `open_question` (marker word
  lists), capped per paper, with excerpt + provenance.
- `build_supervision(topic, papers, docs)` → `Supervision`: prioritized reading
  plan (citations→recency, with rationale + whether ingested), stated gaps,
  open questions (from gaps, else theme-derived), corpus-health advisories
  (e.g. "only N of M ingested", "no recent papers"), and concrete next steps.

### 9.6 Synthesis & bibliography
- `research/synthesis.rs` — `build_synthesis(papers)` → comparison matrix
  (paper × year × venue × citations × themes) + theme-grouped outline where
  every section lists supporting `source_ids` (traceable), plus a cross-cutting
  section for unthemed papers.
- `research/bibliography.rs` — `to_bibtex`/`to_ris` with stable, de-duplicated
  `citation_key` (`surnameYEARword`).

### 9.7 Claim validation & graph (`research/validation.rs`)
- `validate_claims(request)` → `ValidationReport` (hallucinations, citation
  errors, logical issues, bias flags, overall score) by matching draft claims
  against supplied `EvidenceSource`s with a support threshold.
- `build_literature_graph(request)` → nodes/edges/communities from paper
  metadata + concept similarity.

---

## 10. Agents, tools & routing

### Agents (`agent/`)
- Config-driven agent list with a default master agent. Each agent has: id,
  name, model (+ alias tiers like `fast`/`research`), workspace, system prompt,
  allowed channels, **allowed tools** (allow-list), sub-agent allow-list,
  identity, limits (timeout, max concurrent, spawn depth).
- `agent/model.rs` — resolve provider/model refs from config (per request,
  per agent), context windows, costs. `agent/failover.rs` — provider failover
  and (optional) monthly budget enforcement. `agent/model_telemetry.rs` — token/
  cost accounting.
- `agent/session.rs` — `SessionManager` (persisted conversation sessions keyed
  by routing session key; context-window aware). `agent/compaction.rs` —
  safeguard compaction of long histories.
- `agent/subagent.rs` — `SubagentRegistry` (spawn configured sub-agents,
  enforce concurrency/depth). `agent/skills.rs` — skill storage + usage feeding
  Hermes. `agent/runner.rs` — the agent turn loop (prompt → model → tool calls
  → observations → repeat). `agent/prompt.rs`, `identity.rs`, `sandbox.rs`.

### Tool system (`agent/tools.rs`, `agent/tool_policy.rs`, chat tool executor)
- A `ToolDef` registry with JSON-schema-like arg specs. **Tool policy** enforces
  per-agent allow-lists with an `enforce_allowlist` flag — **an empty allow-list
  means deny-all** (fail-closed), not allow-all.
- Tools to implement (names are the contract):
  - Core: `bash`, `read`, `write`.
  - Memory: `memory_search`, `memory_get`, `memory_store`; module memory:
    `module_memory_search`, `module_memory_get`, `module_memory_store`,
    `module_memory_summary`.
  - Study: `srs_card_create`, `srs_generate_from_notes`, `srs_due`,
    `srs_review`, `srs_stats`.
  - Knowledge: `wiki_search`, `graph_search`, `graph_neighbors`.
  - Student pages: `student_page_get`, `student_page_upsert`,
    `student_page_delete`.
  - Messaging/agents: `message_send`, `run_subagent`, `sessions_spawn`.
  - NotebookLM (MCP): `notebooklm_ask`, `notebooklm_setup_auth`,
    `notebooklm_add_notebook`, `notebooklm_add_source`,
    `notebooklm_list_notebooks`, `notebooklm_select_notebook`,
    `notebooklm_get_health`.
  - Web/academic: `web_search`, `scholar_search`.
  - PDFs: `pdf_extract_text`, `pdf_analyze` (native Anthropic/Google PDF input
    when available, else extracted-text analysis).
  - Research: `research_run`, `research_validate_claims`,
    `research_literature_graph`, `research_project_ask`.
  - Documents: `doc_create`, `doc_view`, `doc_list`, `doc_modify`,
    `doc_convert`, `doc_restore`.
  - Slides: `slide_create/read/delete/insert/theme/screenshot/check_overflow/
    check/restore_snapshot/thumbnail_grid/list/build_pptx`.
  - Images: `image_search`, `image_download`, `image_ensure_raster`,
    `image_generate`.
  - LaTeX: `latex_compile`.
  - (Recommended additions for the upgrade: `research_ingest`,
    `research_deep_ask`, `research_gaps`, `research_supervise` as first-class
    tools mirroring the HTTP endpoints.)
- Every tool call is authorized (policy), rate-limited, audited (tool audit
  log), and — for network/execution tools — subject to the security caps.

### Routing (`routing/`)
- `routing.yaml` → `ModuleRouter` maps incoming (channel, account, peer, guild/
  team) to a module + agent + session key. `resolver.rs` resolves agent by id
  and computes routes; `session_key.rs` defines the stable session key format.
- Router is hot-reloadable alongside config.

---

## 11. Channels, scheduler, collab

- `channels/manager.rs` — start/stop channels, status reporting, per-channel
  error state. Channels: `desktop` (in-app) and `telegram` (teloxide polling
  with DM/group allow-lists + streaming mode). Channel messages route through
  the module router and can auto-reply (e.g. Telegram) via the chat pipeline.
- `gateway/scheduler.rs` — recurring jobs from `jobs` config (interval, agent,
  session, message, run-on-startup) that inject agent runs.
- `gateway/collab.rs` — lightweight multi-user collab sessions (create/join/
  get/list, event/state) persisted in the memory store.

---

## 12. Study (SRS), Student OS & Student Planner

### 12.1 Spaced repetition (`study/srs.rs`)
- `SrsService` on the memory store: create card, generate cards from notes
  (deterministic definition/explanation extraction), list due (SM-2-style
  scheduling), review (0–5 rating → next interval), deck stats. Exposed via
  `srs_*` tools and `/api/v1/study/srs/*` endpoints; a `SrsReviewPanel` in the
  desktop app drives the review queue.

### 12.2 Student OS & Planner state store (`student.rs`)
The Student OS and Student Planner pages are **static, editable UIs** whose
state is a set of `key → value` records (mirroring their localStorage). The
gateway owns the canonical copy so agents and the mobile/desktop clients stay
in sync **without changing the page design**.

- `StudentPageStore` over the memory DB with two tables:
  - `student_page_state (page, key, value, updated_at, source, deleted,
    PRIMARY KEY(page,key))` — the record set per page.
  - `student_page_events (id, page, event_type, key, metadata, created_at)` —
    an audit trail (`snapshot_saved`, `record_upserted`, `record_deleted`).
- Pages are normalized to exactly `student-os` and `student-planner`
  (accept aliases `os`/`planner`/underscored forms).
- **Last-write-wins sync:** every upsert/delete/save is conditional on
  `excluded.updated_at >= existing.updated_at`, so offline edits from the
  desktop app and agents merge deterministically by timestamp. Deletes are
  tombstones (`deleted = 1`), not row removal, so sync propagates.
- APIs: `load(page)` → `StudentPageSnapshot { page, records, count,
  updated_at }`, `save_all(page, records, source)`, `upsert_record`,
  `delete_record`, `summarize(page)`.
- **Semantic indexing:** `index_student_page_snapshot(...)` serializes a
  snapshot into a memory entry (tags `student-page`, `<page>`, `planner-state`),
  embeds it, stores/updates it in the vector store, and feeds LM-wiki — so
  agents can *search* planner/OS state, not just fetch it by key.

### 12.3 Agent + client access
- **Tools:** `student_page_get` (page = `student-os` | `student-planner`),
  `student_page_upsert` (create/replace one record by key while preserving the
  UI), `student_page_delete` (tombstone one key).
- **Endpoints:** `POST /api/v1/student/page-state/get`,
  `POST /api/v1/student/page-state/save`.
- **Product wiring:** the **Planner** turns the class-routine image into the
  canonical 7-day class schedule; Module 4 (Routine) reads that schedule and the
  **Student OS** gym/workout plan (plus Telegram/Gmail/CSV) to compile the daily
  routine. Agents must update these via the tools/endpoints and never fork the
  static pages into alternate copies.

### 12.4 Desktop pages
`StudentOSPage` and `StudentPlannerPage` (React) render the editable cards, read
initial state from the gateway (or localStorage offline), and write changes back
through the page-state endpoints so the server copy stays canonical.

---

## 13. Runtime tools & the process guard

- `runtime_tools/process_guard.rs` — `run_guarded_command(GuardedCommandSpec)`:
  async external-command runner with timeout, stdout/stderr byte caps
  (truncation flags), `kill_on_drop`, and — **on Windows** — a Job Object that
  enforces kill-on-close and an optional memory cap. Returns exit code,
  success, timed-out, captured output, and a sandbox report. Reuse this for
  **all** external process execution (OCR, LaTeX, python/node scripts).
- `runtime_tools/latex_compile.rs` — compile LaTeX → PDF into
  `OUTPUT/<dir>/<file>.pdf`; restrict output dirs; enforce write policy.
- `runtime_tools/slide_engine.rs` + `doc_engine.rs` — HTML slide/doc creation,
  screenshots, pptx/docx conversion via Node/Python helper scripts. Use a
  portable interpreter resolver (`python_executable()` honoring `EDUMIND_PYTHON`,
  else `python`/`python3`); never hardcode paths.
- `mcp/` — NotebookLM via MCP (node server) and a Python bridge; health/auth/
  ask/notebook management. Auto-start managed process where configured.

---

## 14. Security model (implement alongside features)

- **Auth** (`gateway/auth.rs`): modes `none` | `token` | `jwt`. Token compares
  a bearer token; JWT validates signature/issuer/audience with leeway. Produce
  an `AuthPrincipal` with roles; admin-only endpoints check `has_role("admin")`.
  Auth config is hot-reloadable (revision counter).
- **Insecure-bind guard**: never serve `auth: none` on a non-loopback address
  without explicit env override (Section 6).
- **CORS/Origin**: enforce `security.allowed_origins`.
- **Action passwords** (`security.rs`): sensitive mutations (config apply,
  memory store, srs mutations, module memory store, …) require an action
  password verified against an `argon2` hash stored at a configurable path;
  min length enforced; can be required/optional per config.
- **Tool audit log**: bounded ring buffer of tool invocations; expose via API.
- **Rate limits**: per-agent/day caps for total, network, and execution tool
  invocations.
- **Execution caps**: max tool timeout, max output bytes, max write bytes,
  process memory cap, Windows job objects toggle.
- **Write sandboxing**: `restrict_tool_writes` + `allowed_tool_write_roots`
  (e.g. `OUTPUT`, `scratch`) — writes outside allowed roots are denied.
- **Secrets** (`secrets.rs`): store provider/channel secrets in the OS keychain
  via `keyring`; redact secrets from any config returned over the API.
- Treat all model/file/web/tool output as untrusted; validate and bound it.

---

## 15. Desktop app (`edumind-tauri`)

- **Stack:** React 18 + Vite, Tauri v2 shell. Bundle fonts + FontAwesome
  locally (offline-capable). ESLint + Playwright configured.
- **Embedded backend — the app IS the backend (UPGRADE — required).** The
  installed desktop app runs the gateway itself; the user never starts a server.
  Two acceptable implementations, in order of preference:
  1. **In-process (preferred).** The Tauri Rust crate (`src-tauri`) depends on
     the `edumind` **library** crate and starts the gateway on a background
     tokio task during app setup: bind `127.0.0.1` on an OS-assigned free port,
     `auth.mode` = token with an app-generated per-launch token, loopback-only.
     Expose the chosen port+token to the frontend via a Tauri command
     (`get_gateway_endpoint`), and shut the gateway down on window close. This
     yields a single process and the simplest install.
  2. **Managed sidecar (fallback).** Bundle the `edumind` binary as a Tauri
     sidecar (`scripts/prepare-sidecar.mjs`, `tauri.sidecar.conf.json`) and have
     the app spawn / health-check / terminate it automatically. Still zero
     manual steps.
  `tauri-bridge.js` abstracts the transport so the UI is identical whether the
  gateway is in-process, a sidecar, or (advanced/opt-in) a remote endpoint.
- **Installers.** Ship real installers via Tauri bundler: Windows `.msi` +
  NSIS `.exe`; ideally macOS `.dmg` and Linux `AppImage`/`.deb`. First-run
  provisions the data dir (`~/.edumind` or OS app-data), generates the action-
  password prompt on first sensitive action, and requires **no** external setup
  (local model/OCR/LaTeX remain optional enhancements detected at runtime).
- **Data & lifecycle.** All SQLite DBs (`memory`, `runs`, `research_projects`,
  `research_fulltext`) live under the per-user app-data dir. The app manages
  gateway lifecycle (start on launch, graceful stop on quit) and surfaces
  gateway health in the UI (ConnectionModal shows embedded status, not a
  connect-to-server flow, by default).
- **Single React app** (no iframe/React duplication). Standardize on React
  components; the Student OS/Planner are editable React "cards" that persist via
  the `student_page_*` tools/endpoints (do not fork into iframe copies).
- **Key components** (`src/components/`): `Sidebar`, `MainContent`,
  `ChatPanel`, `ModulePanel` (the 6 modules), `ResearchWorkspace`,
  `RoutinePlannerPanel`, `StudentOSPage`, `StudentPlannerPage`, `Calendar`,
  `SrsReviewPanel`, `ModuleMemoryPanel`, `AgentsDashboard`,
  `AgentControlPanel`, `ActiveAgentsPage`, `AdminPanel`, `ConnectionModal`,
  `PasswordPrompt`, `DownloadQueue`, `TaskAutomation`, `TranscriptSync`,
  `VaultBrowser`, `ErrorBoundary`. Shared UI in `components/{shared,ui,
  student-ui,student-pages}`.
- **Services/hooks** (`src/services`, `src/hooks`): gateway client (frame
  protocol over HTTP + WS), routine parsing extracted into a testable module,
  design tokens consolidated into CSS `:root`. Accessibility: keyboard-navigable
  controls, ARIA on theme/quick-link controls.
- **Research Workspace UI** must expose the assistant loop: run/refresh, project
  list, ask/deep-ask, ingest (with per-paper status incl. `ocr` flag),
  documents list, gaps, supervise (reading plan + next steps), synthesis, and
  export.
- **Testing:** Playwright e2e (`e2e/app.spec.js`) covering shell render,
  sidebar, module routing, theme switching, offline font bundling, and the
  weekly study workspace. Node safety tests in `scripts/*.test.mjs` (routine,
  scheduler, student-sync). `npm run lint`, `npm run test:e2e`.
- **Do not** run dev servers/watchers as blocking build steps; document the
  commands for the user to run manually.

---

## 15B. 3D graph visualization (UPGRADE — required)

**Every graph in the app is an interactive 3D force-directed graph.** No 2D
node-link diagrams. This covers the knowledge graph, the literature/citation
graph (research), the memory graph, and any community/cluster view.

- **Tech.** Use a WebGL 3D force-graph renderer — recommended
  `react-force-graph-3d` (Three.js under the hood) with `three-forcegraph`;
  Three.js directly is acceptable for custom scenes. Must run offline (bundle
  the deps; no CDN).
- **Shared component.** One reusable `Graph3D` component
  (`src/components/shared/Graph3D.jsx`) consumes the canonical graph payload
  `{ nodes: [{id,label,node_type,properties}], edges/links:
  [{source,target,relation,weight}], communities? }` returned by the gateway
  (`GraphData` from `edumind-core`). All graph surfaces render through it.
- **Interactions.** Orbit/zoom/pan; hover tooltips; click-to-focus + camera fly-
  to; node color by `node_type` (Paper/Author/Concept/Venue/Source) and by
  community; edge thickness by `weight`, label by `relation`; drag nodes;
  search/filter and highlight neighbors; toggle labels; freeze/re-heat layout.
- **Performance.** Handle large graphs: instanced rendering, level-of-detail,
  optional server-side pruning (top-k by weight/degree), and pause simulation
  when idle. Degrade gracefully (fewer effects) on weak GPUs.
- **Data endpoints.** Expose graph payloads for each surface: the knowledge
  graph (`graph_search`/`graph_neighbors` results assembled into `GraphData`),
  the research literature graph (`/research/literature-graph`), and a memory
  graph view. The frontend never computes layout server-side — it only consumes
  nodes/edges and lays out in 3D on the client.
- **Accessibility.** Provide a keyboard-navigable node list + text summary
  alongside the 3D canvas (a table/tree fallback) so the information is not
  WebGL-only. Respect reduced-motion (auto-stop animation).
- **Testing.** Component test that `Graph3D` mounts with a fixture payload,
  renders the expected node/edge counts, and exposes the accessible fallback
  list; Playwright smoke test that each graph surface mounts a canvas.

---

## 15A. Mobile companion app — "MeetMind" (`mobile/`)

An Android app (Kotlin + Jetpack Compose, Material 3) that captures lectures and
feeds their transcripts into EduMind so **Class Notes (Module 1)** can work in
Auto mode. Package `com.example.meetmind`; app label "MeetMind".

### Purpose & flow
1. **Record** lecture audio via a foreground microphone service
   (`service/RecordingService.kt` + `audio/AudioRecorder.kt`). Permissions:
   `RECORD_AUDIO`, `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MICROPHONE`,
   `POST_NOTIFICATIONS`, `INTERNET`. `foregroundServiceType="microphone"`,
   `stopWithTask="false"` so recording survives app backgrounding.
2. **Transcribe** with AssemblyAI (`service/AssemblyAiService.kt`): upload audio
   → submit a transcript job (`speech_models: ["universal-2"]`,
   `language_codes: ["en","bn"]`, `speaker_labels`, `punctuate`, `format_text`)
   → poll until `completed` → format with speaker labels (`Speaker A: ...`).
3. **Sync** the transcript to a backend (`service/SupabaseService.kt`): REST
   `POST {SUPABASE_URL}/rest/v1/meetings` with `apikey` + `Bearer` anon key,
   body `[{ transcript, created_at (UTC ISO) }]`, `Prefer: return=representation`
   → returns the row id.
4. **Ingest into EduMind:** the synced transcript lands in the gateway's module
   memory for the `class-notes` module with `content_type: "transcript"`
   (Module 1 Auto mode fetches it via `module_memory_search` before falling back
   to resource analysis). Document the sync bridge (Supabase → gateway module
   memory) as part of the build.

### Structure
- `MainActivity.kt` — Compose entry; screens in `ui/`: `LoginScreen`,
  `DashboardScreen`, `ResultScreen`; theme in `ui/theme/`.
- `config/ApiConfig.kt` — reads `ASSEMBLYAI_API_KEY`, `SUPABASE_URL`,
  `SUPABASE_ANON_KEY` from `BuildConfig` (injected at build time; **never**
  hardcode keys).
- `model/Models.kt` — DTOs: `TranscriptRequest/Response`, `ProcessResponse`,
  `AnalysisContainer/Detail`, `ActionItem`, `AnalyzeTextRequest`.
- Networking via OkHttp + coroutines; `res/xml/network_security_config.xml`
  with `usesCleartextTraffic=false` (HTTPS only).

### Build/config
- Gradle (Kotlin DSL): `build.gradle.kts`, `settings.gradle.kts`,
  `gradle.properties`, wrapper. Secrets injected via `BuildConfig` fields from
  `local.properties`/CI secrets — keep them out of source control.
- **Upgrade note:** allow pointing sync directly at the EduMind gateway
  (a `/api/v1/modules/class-notes/memory/store` call with
  `content_type:"transcript"`) as an alternative to Supabase, and support
  on-device offline queueing with retry.

---

## 16. Agent operating rules (`Sandbox/AGENTS.md`)

Ship an `AGENTS.md` that defines the agent hierarchy and per-module operating
procedures (this is read by agents at session start):
- Master agent orchestrates; six sub-agent "managers" each own a module and may
  spawn sub-sub-agents (`sessions_spawn`, `runtime: "subagent"`).
- Output convention: all artifacts under `{OUTPUT_DIR}/<Module>/` with
  `{Module}_{Topic}_{YYYY-MM-DD}.{ext}` naming; `{OUTPUT_DIR}` from
  `EDUMIND_OUTPUT_DIR` (never hardcode absolute paths).
- Module 1 (Class Notes): Auto mode fetches transcript from DB (module memory,
  `content_type:"transcript"`); the slides+topper resource-analysis stage always
  runs whether or not a transcript exists; LaTeX `.tex` → compile to PDF.
- Module 4 (Routine): the class schedule comes from the Planner page (do not
  re-OCR the routine image here); personal schedule from gym (Student OS) +
  Telegram + Gmail + CSV; reconcile against the existing saved routine.
- Module 5 (Research): the full assistant/supervisor loop — clarify → run →
  (ask) → **ingest PDFs** → **deep-ask** → **gaps** → **supervise** →
  synthesize → validate → export; record notes/questions on the project. Deep-
  reading endpoints and OCR behavior documented inline.

---

## 17. Testing, CI & quality gates

- **Rust:** every module has `#[cfg(test)]` unit tests. Integration tests under
  `edumind/tests/` for config-example round-trip, gateway auth, memory
  management, security policy, websocket protocol. Target ≥ ~110 passing tests.
- **Determinism:** tests must not require network or external tools. Embeddings
  default to the hash provider in tests; OCR/LaTeX tests only assert graceful
  behavior when tools are absent.
- **CI** (`.github/workflows/ci.yml`): `cargo build --workspace`,
  `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D
  warnings` (clippy is **blocking**). Frontend job: `npm ci`, `npm run lint`,
  `npm run test:e2e` (Playwright).
- Provide a `.gitlab-ci.yml` mirror if targeting GitLab too.

---

## 18. Phased build plan (do these in order)

Each phase must end green (build + tests + clippy).

1. **Workspace skeleton** — Cargo workspace, `edumind-core` with `domain`/`runs`
   types + tests, empty `edumind` lib/bin that compiles, CI wired.
2. **Config + infra** — `EduMindConfig` (all sections, serde defaults + aliases),
   loader/validate/env-expand, `config.example.yaml`, `infra::error`, blocking
   bridge, redaction. Round-trip test of the example config.
3. **Memory core** — SQLite store (+ FTS5), `TextEmbedder` (hash + ollama +
   openai-compatible w/ fallback), vector store + cosine, hybrid search. Tests.
4. **Gateway shell** — protocol frames, `AppState`, router, auth middleware
   (none/token/jwt), CORS/origin, health, insecure-bind guard, WS connect +
   broadcast. Auth + protocol tests.
5. **Agents & tools** — model resolution, session manager, agent/subagent
   registries, tool registry + **fail-closed** tool policy, chat run loop,
   `run_subagent`/`sessions_spawn`, tool audit + rate limits. Policy tests.
6. **Security** — action passwords (argon2), write-root sandboxing, execution
   caps, secrets via keyring, process guard (Windows job objects). Security
   policy tests.
7. **Routing & channels** — `routing.yaml` router (+ hot reload), resolver,
   desktop + Telegram channels, scheduler, collab.
8. **Study, Student OS & Planner** — SRS service + endpoints; `StudentPageStore`
   (state + events tables, last-write-wins sync, tombstone deletes, semantic
   indexing) + `student_page_*` tools + `/student/page-state/*` endpoints. Tests
   for merge-by-timestamp and page normalization.
9. **Research pipeline** — domain types, pipeline engine, native plugins with
   **deterministic** insight/hypothesis analysis, run store, literature
   connectors (Semantic Scholar/PubMed/arXiv/Scopus) + web fallback, claim
   validation, literature graph. Tests for analysis helpers.
10. **Research projects (assistant)** — `ProjectStore`, record-run merge/dedup,
    lexical + semantic ranking, project endpoints (list/get/ask/synthesis/
    export/note/question/scope), synthesis + bibliography modules. Tests.
11. **Full-text deep reading** — `FullTextStore`, chunking, ingest (local +
    HTTPS download, size/timeout caps), `deep-ask` RAG, `documents`. Tests.
12. **OCR** — `tools.ocr` config, `ocr_pdf` via process guard (ocrmypdf +
    sidecar), OCR-aware ingest (`auto`/`force`/`off`, `ocr` flag). Tests for
    disabled/missing-command graceful behavior.
13. **Supervisor** — stated-gap mining, `build_supervision`, `gaps` +
    `supervise` endpoints. Tests.
14. **Memory intelligence** — knowledge graph + communities, LM-wiki, module
    memory, Hermes loop + endpoint, advanced/rerank search.
14b. **Efficient memory (UPGRADE)** — `memory/vector_index.rs`: HNSW ANN index
    (persisted, incremental) + exact fallback, quantized embeddings with exact
    re-rank of ANN candidates, hot/warm/cold tiers with summarization+eviction,
    bounded caches, `memory.index` config + stats endpoint. Deterministic tests
    (ANN==exact on fixtures, quantize round-trip, tier/eviction determinism).
15. **Runtime tools** — LaTeX compile, slide/doc/image engines, MCP/NotebookLM.
16. **Embedded desktop app (UPGRADE)** — Tauri app that **runs the gateway
    in-process** (loopback + per-launch token, free port, `get_gateway_endpoint`
    command, graceful shutdown); real installers (`.msi`/NSIS, ideally
    `.dmg`/AppImage); zero manual backend steps. React app (single approach):
    module panels, Research Workspace (full loop), Student OS/Planner cards, SRS
    review, chat, admin, Playwright e2e + node safety tests, offline fonts, a11y.
16b. **3D graphs (UPGRADE)** — `Graph3D` shared component (react-force-graph-3d/
    Three.js, offline), rendering knowledge/literature/memory/community graphs
    with orbit/hover/focus/filter, performance handling, and an accessible
    fallback list. Component + Playwright smoke tests.
17. **Desktop Student surfaces** — `StudentOSPage` + `StudentPlannerPage` React
    pages (editable cards, gateway-synced via page-state endpoints); Planner
    turns the class-routine image into the canonical 7-day schedule that Module 4
    consumes.
18. **Mobile companion (MeetMind)** — Android record → AssemblyAI transcribe →
    sync; and the bridge that lands transcripts in `class-notes` module memory
    as `content_type:"transcript"` for Module 1 Auto mode.
19. **AGENTS.md + docs** — operating rules, README, run/setup docs.
20. **Hardening** — end-to-end pass on security caps, clippy `-D warnings`,
    docs, example config completeness.

---

## 19. Acceptance criteria (definition of done)

- `cargo build --workspace` and `cargo test --workspace` are green;
  `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- Gateway boots from `config.example.yaml`, serves `/health`, and refuses
  insecure non-loopback `auth: none` binds without the env override.
- A user can: run a research topic → papers persist in a project → **ingest
  PDFs (with OCR fallback for scanned files)** → **deep-ask** returns cited
  passages from paper bodies → **gaps** returns author-stated gaps →
  **supervise** returns a reading plan + next steps → **export** BibTeX/RIS.
- Memory search returns hybrid results; SRS schedules reviews; Telegram (if
  configured) routes and replies; config hot-reloads reloadable changes.
- Tool calls are authorized (fail-closed), rate-limited, audited, write-
  sandboxed, and resource-capped; sensitive mutations require the action
  password.
- Desktop app launches (sidecar or remote), renders the six modules and the
  Research Workspace, persists Student OS/Planner state, and passes Playwright
  e2e + node safety tests.
- Student OS & Planner state round-trips through the gateway with last-write-
  wins merge; editing a card in the app and via `student_page_upsert` converges
  deterministically, and the Planner's 7-day schedule is what Module 4 consumes.
- The MeetMind Android app records a lecture, produces a transcript, and that
  transcript becomes retrievable to Class Notes Auto mode via
  `module_memory_search(module="class-notes", content_type="transcript")`.

### Upgrade acceptance (this version)
- **One-app install:** installing the produced installer and launching it gives
  a fully working app with **no separate backend step**; the gateway runs
  in-process (loopback + auth), and there is no "connect to server" requirement
  for local use. Killing the window stops the gateway cleanly.
- **3D everywhere:** the knowledge graph, research literature graph, and memory
  graph all render in the shared interactive **3D** `Graph3D` component (with the
  accessible fallback), not as 2D diagrams.
- **Efficient memory:** vector search uses the ANN index (with exact fallback);
  ANN top-k matches exact top-k on the test fixtures; embeddings are stored
  quantized with exact re-rank; tiering/eviction and the memory-stats endpoint
  work; large-corpus search is measurably sub-linear vs. the brute-force scan.

---

## 20. Coding standards & constraints

- Idiomatic Rust; small modules with narrow public APIs; prefer pure functions.
- Errors via `thiserror`/`anyhow`; no `unwrap`/`expect` on fallible runtime
  paths (tests may use them).
- Run blocking/CPU work (PDF extract, embedding, OCR orchestration) off the
  async runtime (`spawn_blocking` / the blocking bridge).
- New/optional serialized fields use `#[serde(default)]`; add camelCase
  `alias`es where the desktop app expects them; keep persisted formats
  backward-compatible.
- No hardcoded absolute paths; resolve via config + env + workspace-relative.
- Windows-first correctness for process execution, paths, and sandboxing.
- Network/model features are optional and behind config; the deterministic core
  (and the whole test suite) must work fully offline.
- Flag outbound network operations (PDF downloads, model calls) and enforce
  HTTPS + size/time caps.
- Keep clippy clean under `-D warnings` at all times.

---

## 21. Upgrade opportunities (optional, for a "next version")

Pursue only after the base is green; keep each behind config and tested:
- Promote research deep-reading to first-class tools (`research_ingest`,
  `research_deep_ask`, `research_gaps`, `research_supervise`).
- Section-aware PDF parsing (methods/results/limitations) and table/figure
  extraction feeding `StructuredDocument`.
- Cross-encoder reranking for deep-ask; citation-graph expansion (follow
  references/citations) to grow a project automatically.
- Draft generation (related-work/intro) grounded in the synthesis outline with
  inline citations, then auto-validated by the critic.
- Multi-language OCR presets; incremental re-ingest; a vault browser for
  downloaded PDFs.
- Desktop: live pipeline progress via WS events, per-paper reading tracker, and
  a supervisor "advisor" panel.

---

*End of master build prompt.*
