# AI Assistant Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Persist private assistant dialogs and execute typed actions only after explicit confirmation.
**Architecture:** Rust generates typed proposals from grounded workspace context. PostgreSQL transactions lock proposals and apply validated changes exactly once; JavaScript displays server-generated summaries.
**Tech Stack:** Existing Rust, Axum, SQLx, PostgreSQL, Ollama, vanilla JavaScript.
**Spec:** docs/superpowers/specs/2026-11-12-assistant-design.md

## Global Constraints
- Keep business logic in Rust and the existing web UI.
- Never mutate workspace records while generating a proposal.
- Only the owner can read dialogs or confirm/reject proposals.
- Verify current permissions on every confirmation.
- Roll back the whole operation if any change fails.

## Review Focus
- Concurrent confirmation returns one result and one mutation.
- Cross-user dialog/proposal IDs return 404.
- Untrusted model parameters cannot grant roles, inject SQL or link to external executable URLs.
- A stale preview never silently overwrites manual edits.
- Overnight shifts check neighbors outside the requested week.

### Task 1: Rust action validation and durable schema
**Files:** migrations/007_assistant.sql, src/assistant/actions.rs, src/assistant/mod.rs.
**Interfaces:** Action enum; validate(&Action) -> ApiResult<()>; execute(tx, auth, action) -> ApiResult<Value>.
- [ ] Add failing Rust tests for invalid/unknown actions, excessive batches, duplicate days and task fields.
- [ ] Run cargo test; see expected validation failures.
- [ ] Implement typed actions, schemas and validation; add migration for owned dialogs/messages/proposals.
- [ ] Run cargo test and migration against isolated PostgreSQL.

### Task 2: Grounded planning and transactional confirmation
**Files:** src/assistant/mod.rs, src/assistant/actions.rs, src/main.rs, tests/assistant_integration.py.
**Interfaces:** GET/POST /api/assistant/dialogs; GET /:id; POST /:id/messages; POST /proposals/:id/confirm|reject.
- [ ] Add integration tests for preview-without-write, rejection, single/concurrent confirmation, rollback, ownership, stale data, current role and overnight rest.
- [ ] Run tests before endpoints exist; expect 404.
- [ ] Implement bounded context, Ollama JSON planning, server-rendered exact summaries, serializable transaction executor and ownership-scoped history.
- [ ] Run tests with isolated PostgreSQL and fake Ollama.

### Task 3: Confirmation cards and delivery
**Files:** static/assistant.html, static/assistant.js, static/api.js, static/config.js, BACKEND.md.
**Interfaces:** Message.proposals[] with id/status/details/result; API.confirmProposal and API.rejectProposal.
- [ ] Add browser coverage for exact proposal details, cancel/confirm, loading/error states, switching dialogs while sending and reload.
- [ ] Connect assistant routes to real backend; show proposal cards and working links, prevent duplicate submissions.
- [ ] Run Rust tests, JS syntax checks and browser checks.
- [ ] Package updated source without .env, uploads, git metadata or build artifacts; include .env.example and Russian startup instructions.
