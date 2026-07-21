---
goal: Premium EduMind Platform Upgrade
version: 1.0
date_created: 2026-07-15
last_updated: 2026-07-19
owner: EduMind Engineering
status: Complete
tags: [upgrade, architecture, agents, desktop, education, security]
---

# Introduction

![Status: Complete](https://img.shields.io/badge/status-Complete-brightgreen)

This plan extends `EDUMIND_MASTER_BUILD_PROMPT.md` with premium-grade trust, adaptive study intelligence, reliable agent execution, observable desktop workflows, and measurable quality. Implement this plan after the master prompt's workspace skeleton, configuration, memory core, and gateway shell are green.

## 1. Requirements & Constraints

- **REQ-001**: Preserve the master prompt's local-first architecture: all required study data, indexes, audit records, and evaluation fixtures remain usable without network access.
- **REQ-002**: Add a source-grounded answer contract: every factual answer generated from student material must return citations, evidence spans, confidence, and an explicit `insufficient_evidence` result when evidence is absent.
- **REQ-003**: Add a premium learning loop that calculates mastery, retention risk, workload, and next-best study actions deterministically from Student Planner, SRS, and module-memory data.
- **REQ-004**: Add resumable agent runs with durable checkpoints, idempotency keys, cancellation, budget ceilings, and a user-visible execution timeline.
- **REQ-005**: Add a unified desktop command palette, activity center, onboarding, and recovery flows without duplicating business logic in React.
- **REQ-006**: Add an offline evaluation harness with fixed fixtures for groundedness, routing, memory retrieval, policy enforcement, and critical student workflows.
- **SEC-001**: Encrypt sensitive local records using an OS-keychain-derived key; never persist plaintext provider keys, action passwords, or raw authorization tokens.
- **SEC-002**: Apply least privilege to every tool call using capability grants scoped to run ID, agent ID, module ID, filesystem roots, and expiry.
- **SEC-003**: Treat document text, OCR text, web content, plugin output, and model output as untrusted; record prompt-injection findings and block tool escalation from untrusted content.
- **CON-001**: Keep deterministic tests independent of Ollama, cloud models, OCR binaries, LaTeX, SearXNG, and network access.
- **CON-002**: Maintain Windows-first path and process correctness while retaining macOS and Linux compatibility.
- **GUD-001**: Add new Rust behavior as small pure modules with serde-compatible types, `#[serde(default)]` on persisted optional fields, and unit tests beside each module.
- **PAT-001**: Use `Plan -> Execute -> Verify -> Commit` for all agent runs; a stage transition is valid only after its persisted verification record succeeds.

## 2. Implementation Steps

### Implementation Phase 1

- GOAL-001: Establish premium contracts for trusted retrieval, observable runs, and deterministic learning intelligence.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-001 | Create `crates/edumind-core/src/evidence.rs` with `EvidenceSpan`, `GroundedAnswer`, `EvidenceStatus`, and `Citation`; expose it from `lib.rs`; require non-empty citations when `EvidenceStatus::Grounded`. | Complete | 2026-07-18 |
| TASK-002 | Create `crates/edumind-core/src/learning.rs` with deterministic `MasterySnapshot`, `RetentionRisk`, `StudyRecommendation`, and `LearningSignal` records; expose pure `score_mastery` and `rank_recommendations` functions. | Complete | 2026-07-18 |
| TASK-003 | Extend `crates/edumind-core/src/runs.rs` with `RunCheckpoint`, `RunBudget`, `RunVerification`, and `RunTimelineEvent`; preserve backward compatibility with serde defaults. | Complete | 2026-07-18 |
| TASK-004 | Add SQLite migrations and repositories in `edumind/src/runs.rs` for checkpoints, budget consumption, verification records, and append-only timeline events; make `create_checkpoint` idempotent by `(run_id, stage, attempt)`. | Complete | 2026-07-18 |

Completion criteria: `edumind-core` serializes all new types round-trip; duplicate checkpoint insertion is harmless; all pure scoring tests pass with fixed clock and fixtures.

### Implementation Phase 2

- GOAL-002: Enforce trustworthy agent execution and evidence-first research responses.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-005 | Create `edumind/src/agent/run_engine.rs` implementing `PlanExecuteVerifyCommitEngine`; persist a checkpoint before and after each stage, enforce `RunBudget`, and expose cancellation through a `CancellationToken`. | Complete | 2026-07-19 |
| TASK-006 | Create `edumind/src/agent/capabilities.rs` with signed in-memory `CapabilityGrant` validation for tool name, agent, run, module, allowed write roots, expiry, and mutation permission; default to deny. | Complete | 2026-07-19 |
| TASK-007 | Create `edumind/src/security/content_guard.rs` to classify prompt-injection patterns in untrusted input; return `ContentRisk` and prohibit content-derived tool names, paths, or privilege elevation. | Complete | 2026-07-19 |
| TASK-008 | Update `edumind/src/research/` deep-ask and synthesis handlers to construct `GroundedAnswer` from retrieved chunks and return `insufficient_evidence` instead of unsupported claims. | Complete | 2026-07-19 |
| TASK-009 | Extend `edumind/src/gateway/` endpoints with `POST /api/v1/runs/{run_id}/cancel`, `GET /api/v1/runs/{run_id}/timeline`, and `GET /api/v1/runs/{run_id}/evidence`; emit WebSocket events for each state transition. | Complete | 2026-07-19 |

Completion criteria: an unauthorized tool call is denied; cancelled runs do not execute later stages; evidence-less deep-ask responses return `insufficient_evidence`; timeline events survive gateway restart.

### Implementation Phase 3

- GOAL-003: Deliver adaptive learning orchestration backed by efficient local memory.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-010 | Create `edumind/src/study/learning_engine.rs` to read SRS reviews, Planner schedule, Student OS availability, and module-memory retrievals; call the Phase 1 scoring functions and persist a daily `MasterySnapshot`. | Complete | 2026-07-19 |
| TASK-011 | Add `GET /api/v1/study/insights` and `POST /api/v1/study/recommendations/refresh` handlers in `edumind/src/gateway/study.rs`; return ranked actions with rationale and planner-conflict evidence. | Complete | 2026-07-19 |
| TASK-012 | Extend `edumind/src/memory/vector_index.rs` with retrieval provenance fields (`candidate_rank`, `exact_rerank_score`, `index_generation`) and ensure quantized ANN candidates are exact-reranked before evidence construction. | Complete | 2026-07-19 |
| TASK-013 | Create `edumind/src/memory/privacy.rs` to implement data classification, encrypted-at-rest payload envelopes, and secure deletion records; retrieve encryption material only through `edumind/src/secrets.rs`. | Complete | 2026-07-19 |
| TASK-014 | Add `edumind/src/jobs/learning_refresh.rs` to schedule one bounded daily refresh and an explicit manual refresh; do not run model calls unless configuration enables them. | Complete | 2026-07-19 |

Completion criteria: identical fixtures produce identical recommendations; the highest-risk overdue concept ranks first; vector evidence includes provenance; encrypted records cannot be decoded with a different key.

### Implementation Phase 4

- GOAL-004: Make the desktop experience feel premium, transparent, fast, and accessible.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-015 | Create `edumind-tauri/src/components/shared/CommandPalette.jsx` and `edumind-tauri/src/services/commands.js`; register keyboard actions for modules, search, study review, active runs, and settings using one command schema. | Complete | 2026-07-19 |
| TASK-016 | Create `edumind-tauri/src/components/RunTimelinePanel.jsx` and `edumind-tauri/src/hooks/useRunTimeline.js`; render persisted timeline events, checkpoints, budget use, cancellation, verification, and evidence links from the gateway. | Complete | 2026-07-19 |
| TASK-017 | Create `edumind-tauri/src/components/StudyInsightsPanel.jsx`; display mastery, retention risk, recommended actions, planner conflicts, explanations, and an accessible text alternative for each chart. | Complete | 2026-07-19 |
| TASK-018 | Create `edumind-tauri/src/components/OnboardingFlow.jsx` with local-model detection, privacy controls, workspace choice, optional integrations, and a deterministic offline demo dataset; persist completion state through the gateway. | Complete | 2026-07-19 |
| TASK-019 | Update `edumind-tauri/src/components/shared/Graph3D.jsx` to show evidence-aware tooltips, a selected-node source panel, reduced-motion behavior, GPU fallback, and a keyboard navigable node/evidence list. | Complete | 2026-07-19 |

Completion criteria: command palette actions are keyboard-only operable; a recovered run renders after app restart; every visual insight has a textual alternative; no frontend component accesses storage directly.

### Implementation Phase 5

- GOAL-005: Gate releases with offline evaluation, telemetry, resilience tests, and premium acceptance criteria.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-020 | Create `edumind/tests/fixtures/premium_eval/` containing sanitized notes, PDFs-as-text, planner data, malicious instructions, run failures, and expected evidence/recommendation JSON. | Complete | 2026-07-19 |
| TASK-021 | Create `edumind/tests/premium_evaluation.rs` that measures citation coverage, evidence precision, policy denial rate, deterministic recommendation order, cancellation recovery, and vector provenance using only local fixtures. | Complete | 2026-07-19 |
| TASK-022 | Create `edumind/src/infra/telemetry.rs` with local-only structured metrics for latency, retrieval quality, run failures, cache behavior, and feature usage; redact content and identifiers before persistence. | Complete | 2026-07-19 |
| TASK-023 | Add Playwright cases in `edumind-tauri/e2e/premium-workflows.spec.js` for first-run onboarding, command palette navigation, evidence inspection, cancellation/recovery, study recommendation acceptance, and 3D graph fallback. | Complete | 2026-07-19 |
| TASK-024 | Update `.github/workflows/ci.yml` to run Rust formatting, build, test, clippy, evaluation thresholds, frontend lint, and Playwright; fail CI when thresholds in `edumind/tests/fixtures/premium_eval/thresholds.json` are unmet. | Complete | 2026-07-19 |

Completion criteria: CI blocks regressions below 95% citation coverage on grounded fixtures, any capability-policy bypass, non-deterministic fixture ranking, or inaccessible critical workflow.

## 3. Alternatives

- **ALT-001**: Use cloud-only observability and evaluation services. Rejected because it violates the local-first and offline-test requirements.
- **ALT-002**: Let models self-report confidence without retrieved evidence. Rejected because confidence alone does not make student guidance auditable.
- **ALT-003**: Implement resumable runs only in the React client. Rejected because desktop crashes or restarts would lose authoritative execution state.

## 4. Dependencies

- **DEP-001**: The master prompt's `edumind-core`, SQLite migrations, gateway protocol, authentication, memory, SRS, Student OS, Planner, and Tauri foundations must be implemented first.
- **DEP-002**: Add `tokio-util` for `CancellationToken`, `aes-gcm` for encrypted payload envelopes, and `zeroize` for short-lived sensitive buffers; pin exact compatible versions in `edumind/Cargo.toml`.
- **DEP-003**: `react-force-graph-3d`, `three`, React 18, Tauri v2, and Playwright remain the frontend integration base.

## 5. Files

- **FILE-001**: `D:\open ai hackathon\EDUMIND_MASTER_BUILD_PROMPT.md` is the base specification this upgrade augments without replacing.
- **FILE-002**: `crates/edumind-core/src/evidence.rs`, `learning.rs`, and `runs.rs` define durable premium contracts.
- **FILE-003**: `edumind/src/agent/run_engine.rs`, `capabilities.rs`, and `edumind/src/security/content_guard.rs` implement trusted execution.
- **FILE-004**: `edumind/src/study/learning_engine.rs`, `edumind/src/memory/privacy.rs`, and `edumind/src/infra/telemetry.rs` implement adaptive, private, observable behavior.
- **FILE-005**: `edumind-tauri/src/components/` and `edumind-tauri/e2e/` contain the premium desktop surfaces and user-level tests.

## 6. Testing

- **TEST-001**: Unit-test evidence contracts, learning scores, checkpoint idempotency, budget enforcement, capability grants, content guard decisions, encryption envelopes, and recommendation ranking.
- **TEST-002**: Integration-test run cancellation/recovery, restart persistence, deep-ask evidence behavior, vector exact-rerank provenance, and secure deletion records.
- **TEST-003**: Execute the offline premium evaluation fixture suite and enforce the thresholds declared by TASK-024.
- **TEST-004**: Execute Playwright premium workflow tests with the embedded gateway and network-disabled fixture mode.

## 7. Risks & Assumptions

- **RISK-001**: Evidence requirements can reduce answer coverage on sparse material; mitigate by returning actionable `insufficient_evidence` guidance and capture prompts.
- **RISK-002**: Encrypted local records can become unavailable if OS keychain material is removed; mitigate with explicit recovery/export design before enabling encryption by default.
- **RISK-003**: Timeline and telemetry writes can increase SQLite contention; mitigate with batched append-only writes and bounded retention.
- **ASSUMPTION-001**: The master prompt's phases 1 through 4 are complete before Phase 1 of this plan begins.
- **ASSUMPTION-002**: Student Planner and SRS records include stable timestamps and IDs needed for deterministic learning scoring.

## 8. Related Specifications / Further Reading

- `D:\open ai hackathon\EDUMIND_MASTER_BUILD_PROMPT.md`
- [Tauri v2 documentation](https://v2.tauri.app/)
- [OWASP LLM Prompt Injection Prevention Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/LLM_Prompt_Injection_Prevention_Cheat_Sheet.html)
- [NIST AI Risk Management Framework](https://www.nist.gov/itl/ai-risk-management-framework)
