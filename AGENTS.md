# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` holds the entire service: HTTP routes, ingest pipeline, indexing, and SQLite interactions.
- `Cargo.toml` declares dependencies and Rust edition (2024).
- `data/` is created at runtime and stores `data/app.db` (SQLite) plus `data/index/` (Tantivy index).
- `target/` is Cargo build output.

## Build, Test, and Development Commands
- `cargo build`: Compile the service.
- `cargo run`: Build and start the API server on `0.0.0.0:3000`.
- `cargo test`: Run tests (none currently defined).
- `cargo fmt`: Format code with rustfmt.
- `cargo clippy`: Lint for common Rust issues.

## Coding Style & Naming Conventions
- Indentation: 4 spaces (Rust standard).
- Naming: `snake_case` for functions/variables, `CamelCase` for types.
- Manage dependencies with `cargo add` rather than editing `Cargo.toml` directly.
- Prefer explicit error contexts via `anyhow::Context` when adding new fallible calls.
- Keep async boundaries clear; avoid blocking calls in request handlers.

## Testing Guidelines
- No test framework or tests are present yet.
- If adding tests, place unit tests alongside modules (e.g., `src/main.rs`) or add integration tests under `tests/`.
- Use descriptive names like `test_ingest_rejects_empty_urls`.

## Commit & Pull Request Guidelines
- No Git history is present, so no established commit convention.
- Use concise, imperative commit messages (e.g., "Add ingest validation").
- PRs should include:
  - Summary of changes and rationale.
  - API behavior changes or new endpoints.
  - Any manual testing performed (commands and results).

## Security & Configuration Tips
- The server accepts URLs for ingestion; validate and normalize inputs consistently.
- `data/` contains persisted content; avoid committing it.
- Keep request body size limits in mind (`2MB` limit in the server).
