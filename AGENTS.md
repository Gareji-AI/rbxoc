# AGENTS.md

Guidance for coding agents working in the **rbxoc** repository (Roblox Open Cloud CLI).

## Quick commands

Run from the repo root after `mise install` (Rust **1.99** pinned in `mise.toml`).

```bash
mise run check          # fmt --check, clippy -D warnings, tests
mise run verify         # check + release build + scripts/check-coverage.mjs
mise run similarity     # optional: similarity-rs on src/ (dev hygiene, not CI)
cargo build --release   # CI also builds release and runs catalog contract tests
```

Do not commit API keys, OAuth tokens, or live universe/place IDs invented for tests.

## Project overview

- **Purpose:** Agent-first CLI for Roblox Open Cloud. Every command returns a stable JSON envelope (`ok`, `command`, `summary`, `data`, `hints`, `error`).
- **Source of truth for endpoints:** `src/catalog.rs` — add or change catalog entries before implementing HTTP paths. Run `rbxoc api list` / `api show <id>` to inspect.
- **HTTP + auth:** `src/auth.rs`, `src/ctx.rs`.
- **Errors and permission hints:** `src/open_cloud_errors.rs`; use `failure_for_endpoint(..., endpoint_id, ...)` for API failures so 401/403 include Creator Dashboard scope hints from the catalog.
- **Subcommands:** `src/cli.rs` dispatches to `src/cmd/*.rs`.
- **Docs:** `docs/open-cloud-coverage.md`, `docs/live-api-verification.md`.

## Architecture rules

1. **Catalog first.** New Open Cloud surface → new `Endpoint` in `catalog.rs` with `id`, scopes, `auth`, `implemented`, and `note`. Keep `scripts/check-coverage.mjs` passing after changes.
2. **Output contract.** Success and failure go through `output::Output` / `ErrorBody`. Prefer stable `error.code` values; extend `open_cloud_errors.rs` when Roblox returns structured enrollment/OAuth cases.
3. **Credentials are project-local.** Resolution order: `--api-key` → `RBXOC_API_KEY` → `--api-key-file` → walk up to first `.rbxoc/api-key`. OAuth: `RBXOC_OAUTH_TOKEN` or `.rbxoc/oauth-token`. No home-directory fallback (keys are universe-scoped).
4. **Multipart uploads.** Bodies that cannot `try_clone()` must still send once (`ctx.rs`); do not require clone for retry on streaming uploads.
5. **English** for user-facing CLI strings, README, and agent docs in this repo.

## Coding standards

- **Edition:** Rust 2024; `rust-version = "1.99"` in `Cargo.toml`.
- **Format / lint:** `cargo fmt`; `cargo clippy --all-targets -- -D warnings` (warnings are errors).
- **Tests:** Unit tests beside modules (`#[cfg(test)]`). Prefer testing path builders, parsers, and hint text without live API calls.
- **Dependencies:** Keep the dependency set minimal; no Roblox toolchain pins in `mise.toml` unless a task needs them.
- **Secrets:** Never log or commit credentials. Mask in summaries (`auth.rs` patterns). Do not add `~/.rbxoc` home fallback.

## Testing and CI

GitHub Actions (`.github/workflows/ci.yml`): `cargo fmt --check`, clippy, test, release build, catalog JSON smoke test, exit codes **2** (`auth_missing`) and **64** (unknown endpoint).

When changing CLI behavior, run locally:

```bash
mise run verify
```

## Git and repo hygiene

- **Do not track:** `.rbxoc/`, `.ralph/`, `.pi/`, `target/`.
- **Do not commit** local agent transcripts, sweep scripts output, or `.env` from consumer projects.
- Release versioning lives in `Cargo.toml`; tag releases as `vX.Y.Z` on GitHub when publishing to crates.io.

## Boundaries (do not do without explicit ask)

- Force-push `main`, rewrite published tags, or publish to crates.io without maintainer intent.
- Call live Open Cloud with real credentials in CI or in committed scripts.
- Expand scope into unrelated Roblox tooling (Rojo, Studio automation) inside this crate.
- Duplicate long API documentation in AGENTS.md — link to `README.md` and `docs/` instead.

## Live API verification (maintainers)

Use a **real** universe ID and project-local `.env` / `.rbxoc` in a consumer repo (e.g. game template), not invented IDs. See `docs/live-api-verification.md` for known platform limits (OAuth-only analytics metrics, PerformanceMonitoringV2 enrollment for alerts, place publish 409 busy).
