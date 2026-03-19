# Phase 1: Shared `hstack-core` Crate — LLM Provider Abstraction + Local Ticket Generation

Port the LLM provider switching system from [serving.py](file:///Users/antoine/Documents/production/aimapping/src/compute/models/serving.py) to a Rust crate (`hstack-core`) shared between the Tauri desktop app and the future Axum server. This enables **offline ticket generation** via local LLMs and **user-configurable LLM providers**.

## User Review Required

> [!IMPORTANT]
> **Workspace restructure**: This proposes converting the repo to a Cargo workspace with `crates/hstack-core` and `crates/hstack-app` (Tauri). The existing `frontend/src-tauri` Cargo project moves to `crates/hstack-app`. The Python server stays untouched for now — it keeps running in parallel until Phase 2 replaces it with Axum.

> [!WARNING]
> **API key storage**: User-provided API keys will be stored locally on disk (encrypted via Tauri's `tauri-plugin-store`). No keys are ever sent to the HStack server. This is the standard approach for desktop apps, but worth confirming you're comfortable with it.

---

## Proposed Changes

### Workspace Root

#### [NEW] [Cargo.toml](file:///Users/antoine/Documents/perso/HStack/Cargo.toml)
Top-level Cargo workspace manifest:
```toml
[workspace]
members = ["crates/hstack-core", "crates/hstack-app"]
resolver = "2"
```

---

### `hstack-core` (shared library crate)

The core of this change. A pure Rust library with no Tauri or Axum dependencies — usable from both.

#### [NEW] [Cargo.toml](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/Cargo.toml)
Dependencies: `reqwest`, `serde`/`serde_json`, `tokio`, `thiserror`, `tracing`, `uuid`, `chrono`.

#### [NEW] [src/lib.rs](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/src/lib.rs)
Public modules: `provider`, `chat`, `ticket`, `settings`, `error`.

#### [NEW] [src/provider/mod.rs](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/src/provider/mod.rs)
Core types ported from `primitives.py`:

| Python | Rust |
|--------|------|
| `ServingType` enum | `ProviderKind` enum (`OpenAiCompatible`, `Gemini`) |
| `Serving` model | `ProviderConfig` struct (endpoint, api_key, model_name, kind) |
| `RateLimitConfig` | `RateLimitConfig` struct (rps, rpm, tpm) — included from Phase 1 for future parallel/batch actions |
| `ModelName` enum | `String` field on config (user picks any model name) |

Key difference from the Python version: **no hardcoded model registry**. Users configure providers dynamically (name, endpoint, API key, kind). The dispatch uses `ProviderKind`:

```rust
pub enum ProviderKind {
    OpenAiCompatible,  // Covers OpenAI, Mistral, Groq, Ollama, any /v1/chat/completions endpoint
    Gemini,            // Google AI Studio / Vertex (uses generateContent REST API)
}
```

#### [NEW] [src/provider/openai_compat.rs](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/src/provider/openai_compat.rs)
Direct port of [_openai.py](file:///Users/antoine/Documents/production/aimapping/src/compute/models/providers/_openai.py) `generate_openai_content` — HTTP POST to `/v1/chat/completions` via `reqwest`. Handles:
- Message formatting, tool schemas
- Response parsing (choices → message → content / tool_calls)
- Exponential backoff retry (port of `retry_on_fail`)

#### [NEW] [src/provider/gemini.rs](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/src/provider/gemini.rs)
HTTP POST to `https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={API_KEY}`. Gemini uses its own function calling format (`functionCall`/`functionResponse` in `parts`) which differs from OpenAI's `tool_calls` — this module handles the translation.

> [!NOTE]
> Gemini 3+ models enforce **Thought Signatures** — encrypted representations of internal reasoning that must be preserved across multi-turn function calling. Missing signatures trigger a 400 error. The implementation will forward these opaque tokens between turns.

Ollama is **not** a separate provider — it exposes an OpenAI-compatible endpoint at `http://localhost:11434/v1/chat/completions` and is handled as `ProviderKind::OpenAiCompatible` with no auth headers.

#### [NEW] [src/chat.rs](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/src/chat.rs)
The tool-calling orchestration loop — port of the `chat_with_gemini()` function from [main.py](file:///Users/antoine/Documents/perso/HStack/hstack/main.py). Takes a user message + provider config + list of tool schemas → runs the generate → extract function calls → execute → return results loop. Returns a `ChatResult` with actions taken + confirmation text.

#### [NEW] [src/ticket.rs](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/src/ticket.rs)
Ticket types and creation logic. Port of [models.py](file:///Users/antoine/Documents/perso/HStack/hstack/models.py) + the tool dispatch handlers from `main.py`:
- `TicketType` enum (Task, Habit, Event, Commute, Countdown)
- `TicketStatus` enum (Idle, InFocus, Completed, Expired)
- `Ticket` struct with payload as `serde_json::Value`
- Tool schema declarations (equivalent to `ai_tools.py` function schemas)

#### [NEW] [src/provider/rate_limit.rs](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/src/provider/rate_limit.rs)
Port of `RateLimitConfig` + token-bucket rate limiter. Tracks rps/rpm/tpm via atomic counters with sliding window. Used by the dispatch layer to throttle provider calls — essential for batch/parallel tool execution.

#### [NEW] [src/settings.rs](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/src/settings.rs)
User settings types:
- `UserSettings` struct (list of configured providers, default provider ID, local processing preference)
- `SavedProvider` struct (name, kind, endpoint, model, encrypted API key reference)
- Serialization to/from JSON for persistence (the storage backend is injected — Tauri uses its store plugin, the server uses a DB)

#### [NEW] [src/error.rs](file:///Users/antoine/Documents/perso/HStack/crates/hstack-core/src/error.rs)
Unified error type via `thiserror`.

---

### `hstack-app` (Tauri integration)

#### [MODIFY] [Cargo.toml](file:///Users/antoine/Documents/perso/HStack/frontend/src-tauri/Cargo.toml)
- Move to `crates/hstack-app/Cargo.toml` (or update path)
- Add `hstack-core = { path = "../hstack-core" }` dependency
- Add `tauri-plugin-store` for persisting user settings + API keys
- Add `tokio` runtime features

#### [MODIFY] [lib.rs](file:///Users/antoine/Documents/perso/HStack/frontend/src-tauri/src/lib.rs)
Replace the `greet` scaffold with Tauri commands that expose `hstack-core`:
- `chat_local` — processes a message via the locally-configured LLM provider
- `get_settings` / `save_settings` — CRUD for provider configuration
- `list_providers` — returns configured providers
- `test_provider` — sends a test message to verify a provider config works

---

## Verification Plan

### Automated Tests

**Unit tests in `hstack-core`** (run with `cargo test -p hstack-core`):

1. **Provider config serialization** — round-trip `ProviderConfig` through `serde_json`
2. **OpenAI-compat response parsing** — test with mock JSON responses (choices/tool_calls/content)
3. **Gemini response parsing** — test with mock Gemini `generateContent` response format
4. **Tool schema generation** — verify tool schemas match expected JSON structure
5. **Chat loop logic** — mock the HTTP layer, test that tool calls are dispatched and results returned
6. **Ticket creation from tool calls** — test that `create_ticket` args produce correct `Ticket` structs
7. **Settings serialization** — round-trip `UserSettings` through JSON

These tests will use mock HTTP responses (no real API calls). I'll write them alongside each module.

**Integration smoke test** (run with `cargo test -p hstack-core --features integration`):

8. **Live Ollama test** (gated behind `#[cfg(feature = "integration")]`) — if Ollama is running locally, send a simple prompt and verify we get a response. This test is optional and skipped in CI.

### Manual Verification

Since this is a new library crate with no UI integration yet (the Tauri commands come at the end), the primary verification is:

1. `cargo build --workspace` — the workspace compiles cleanly
2. `cargo test -p hstack-core` — all unit tests pass
3. `cargo clippy --workspace` — no warnings
4. **After Tauri integration**: `npm run tauri dev` from `frontend/` — app still launches without regression

> [!NOTE]
> I'd appreciate your input on whether you'd like me to also set up a simple **settings UI** in the React frontend as part of this phase, or keep Phase 1 purely backend-focused so we can validate the Rust architecture first.
