# Agent Guide for webhook-rs

This repo is a Rust workspace with three crates:
- `webhook` (shared library)
- `webhook-server` (Axum server)
- `webhook-client` (websocket client)

No Cursor rules or Copilot instructions were found in this repo.

## Build, Lint, Test

Run all builds (workspace):
- `cargo build`

Build a specific crate:
- `cargo build -p webhook`
- `cargo build -p webhook-server`
- `cargo build -p webhook-client`

Release build (used by README):
- `cargo build -p webhook-server --release`
- `cargo build -p webhook-client --release`

Run all tests (workspace):
- `cargo test`

Run tests for one crate:
- `cargo test -p webhook`
- `cargo test -p webhook-server`
- `cargo test -p webhook-client`

Run a single test by name:
- `cargo test -p webhook test_valid_webhook_path`
- `cargo test -p webhook test_jwt_token`
- `cargo test -p webhook test_read_buffer_header`
- `cargo test -p webhook test_read_buffer_header_with_body`

Run a single test by module path (more precise):
- `cargo test -p webhook utils::tests::test_valid_webhook_path`
- `cargo test -p webhook token::tests::test_jwt_token`
- `cargo test -p webhook message::tests::test_read_buffer_header`

Format (rustfmt):
- `cargo fmt`

Lint (clippy):
- `cargo clippy --workspace --all-targets`

Optional: format check only:
- `cargo fmt -- --check`

## Repo Structure and Conventions

Crate layout:
- `webhook` exposes shared types and helpers (`error`, `message`, `queue`, `token`, `utils`).
- `webhook-server` owns HTTP/websocket server and request forwarding logic.
- `webhook-client` owns websocket client and target forwarding.

Error handling:
- Prefer the shared `webhook::Result<T>` and `webhook::Error` types.
- Convert errors into `Error` via `Err(msg.into())` or `Error::XxxError(...)`.
- Use descriptive, human-readable error strings (they are surfaced to users).
- Avoid `unwrap()` in production code; use `?`, `match`, or explicit error mapping.
- `unwrap()` is acceptable in tests or for invariants that are already validated.

Async patterns:
- Use `tokio` and `async fn` for IO and concurrency.
- Long-lived tasks are spawned with `tokio::spawn` and coordinated with `Arc` + `Mutex`.
- Keep lock scopes tight; release locks before awaiting other futures.

## Code Style Guidelines

Formatting:
- Use rustfmt defaults (no custom `rustfmt.toml` in repo).
- Indentation is 4 spaces; line breaks follow rustfmt.

Imports:
- Group imports by source:
  1. `std` imports
  2. external crates
  3. internal modules (`crate::`, `webhook::`)
- Use `use std::{...}` to group related std imports.
- Avoid unused imports; rustfmt and clippy should stay clean.

Types and naming:
- Types, structs, enums: `CamelCase`.
- Functions, methods, variables: `snake_case`.
- Constants: `UPPER_SNAKE_CASE` (example: `RUST_LOG`).
- Prefer explicit types at public boundaries and for struct fields.
- Prefer `Arc<Config>` when sharing immutable config across tasks.

Error types and results:
- Prefer returning `Result<T>` from fallible helpers.
- Convert external errors to `Error::XxxError(String)` with `to_string()`.
- Avoid leaking external error types from public functions.

Data handling and parsing:
- Use explicit parsing with validation steps (see `config.rs`).
- Avoid silent defaults when invalid config is provided; return a clear error.
- For protocol parsing, keep strict validation (see `message.rs`).

HTTP/Websocket behavior:
- The server only allows one connected client at a time.
- Client and server exchange binary frames using the `TunnelMessage` format.
- When adding or changing tunnel message structure, update both client and server.

Logging:
- Use `tracing` macros (`info!`, `error!`) for runtime logs.
- Messages should be concise and useful for diagnosing connection or forwarding issues.
- `RUST_LOG` defaults are set in `webhook-server/src/main.rs` and `webhook-client/src/main.rs`.

Testing style:
- Unit tests live in the same module with `#[cfg(test)]`.
- Test names are descriptive and map to behaviors (see `utils.rs`, `token.rs`, `message.rs`).

Safety and correctness:
- Prefer explicit checks to avoid panics in runtime paths.
- Use `expect()` only when a failure indicates a programmer error.
- Keep websocket ping and stale-connection logic consistent between client and server.

## Common Patterns in This Repo

Configuration loading:
- `Config::build(path)` reads TOML and validates fields.
- Return `ConfigReadError`, `ConfigParseError`, or `ConfigInvalidError` as needed.

Message flow:
- Server: enqueue request -> wait for response -> respond to HTTP caller.
- Client: dequeue request -> forward to target -> send response back.

Queue/map usage:
- `MessageQueue` is FIFO for requests.
- `MessageMap` is keyed by request id for responses with timeout logic.

## Notes for Agents

- No custom Cursor or Copilot rules were found.
- Keep edits minimal and consistent with existing style.
- If you add new public APIs in `webhook`, update re-exports in `webhook/src/lib.rs` if needed.
- If you add new config fields, update README examples as appropriate.
