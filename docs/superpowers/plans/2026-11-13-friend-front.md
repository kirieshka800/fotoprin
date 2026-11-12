# Friend Front Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Integrate the supplied frontend design without losing existing features.
**Architecture:** Preserve current API and handlers; isolate presentation helpers in interface.js.
**Tech Stack:** Rust/Axum, PostgreSQL, HTML/CSS/JavaScript, Playwright.
**Spec:** docs/superpowers/specs/2026-11-13-friend-front-design.md

## Global Constraints

No frontend build requirement; preserve Rust API, auth, confirmation, deletion and real audio.
Serve the merged frontend as the default UI, without a separate demo version.

## Review Focus

- Dynamic option lists and programmatic/reset values stay in sync.
- Removed modal controls leave no open menus or detached focus targets.
- Checkbox label clicks do not navigate table rows.
- Dark theme works with auth, deletion, AI cards, and cross-tab persistence.
- Effects preserve popovers and mobile page layout.

### Task 1: Visual integration

Files: static/*.html, styles.css, ui.js, interface.js; tests/frontend_browser.py.
Interfaces: preserve API.*, renderShell and page handlers; expose theme/select helpers.

- [x] Inspect archive/current files; write browser scenarios before integration.
- [x] Observe theme control missing in the original UI (Playwright assertion failure).
- [x] Merge presentation while retaining current API/config/page logic.
- [x] Run frontend_browser.py against the real Rust app: themes/cross-tab, async and
  keyboard selects, filters/reset, modal editing/cleanup, checkbox, search/logout,
  desktop/mobile smoke in both themes.

### Task 2: Audio and regressions

Files: static/meeting.html, tests/audio_browser.py, README.md, TESTING_RU.md, CHECKS.md.
Interfaces: real audio element and existing seekTo, paintPlaying, retry/error handling.

- [x] Port volume/mute/skip buttons onto real audio implementation.
- [x] Extend audio_browser.py to verify controls and unavailable-audio disabling.
- [x] Run assistant, audio, deletion and frontend browser scenarios and JS parsing.
- [x] Update docs and independent review; fix Escape propagation and participant menu stacking with browser regressions.
- [x] Upload and verify main.
