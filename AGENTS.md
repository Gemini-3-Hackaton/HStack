# HStack Agent Rules

These rules are mandatory for human contributors and AI coding agents working in this repository.

## Core Standards

1. Fail closed.
   Missing config, invalid auth, malformed payloads, or unavailable remote dependencies must return an explicit error. Do not add insecure defaults, silent fallbacks, or development backdoors.

2. `unwrap` and `expect` are forbidden in repository Rust source.
   The crates in this repo deny `clippy::unwrap_used` and `clippy::expect_used`. Keep new code compatible with that policy, including tests.

3. The browser must not own sync transport.
   Websocket lifecycle, authentication, reconnect logic, and pending-action flushing belong in the Tauri Rust layer. The frontend is for rendering state and sending intent through commands.

4. Sync state is Rust-owned.
   Canonical local sync state lives in the Tauri stores (`base_state.json` and `pending_actions.json`). Do not introduce parallel browser-only task or sync-history stores.

5. Sync must be authenticated and user-scoped.
   Never trust a user ID from the UI by itself. All remote sync operations must be tied to authenticated session data and scoped server-side.

## Architectural Guardrails

1. Frontend responsibilities:
   Render tasks, react to Tauri events, invoke Tauri commands, collect user input.

2. Tauri Rust responsibilities:
   Persist base state and pending actions, own websocket/runtime transport, reconcile server state, emit task/status events to the UI.

3. Server responsibilities:
   Authenticate sync sessions, scope all mutations to the authenticated user, acknowledge only committed writes, and emit explicit remote state change notifications.

## Review Checklist

Before merging sync-related work, verify all of the following:

1. No browser `new WebSocket(...)` exists in application code.
2. No Rust `unwrap` / `expect` was introduced.
3. Remote sync still works with reconnects and explicit error reporting.
4. Pending actions remain durable across app restarts.
5. The UI updates through Tauri commands/events, not browser-local shadow state.

## If You Need To Bend A Rule

Stop and document the reason in the PR or commit discussion first. Do not quietly add exceptions.