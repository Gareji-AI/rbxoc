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
3. **Credentials are project-local.** Resolution order: `--api-key` → `RBXOC_API_KEY` → `--api-key-file` → `RBXOC_OAUTH_TOKEN`. `RBXOC_API_KEY` is the process environment, or the nearest `.env` loaded by dotenvy before parsing when that variable is unset (same load as Asphalt; an existing variable is not overridden). No home-directory fallback (keys are universe-scoped).
4. **Multipart uploads.** Bodies that cannot `try_clone()` must still send once (`ctx.rs`); do not require clone for retry on streaming uploads.
5. **English** for user-facing CLI strings, README, and agent docs in this repo.
6. **Replay safety.** `Ctx::send` retries only safe HTTP methods (GET/HEAD/OPTIONS/TRACE). All other methods are sent once, even when the body is cloneable. Transport/429/5xx outcomes return `mutation_outcome_unknown`; reconcile the target or receipt before resubmitting. Keep publication-specific `send_once` receipt recovery intact.
7. **Failure hints.** Construct failures through `ctx::failure` / `failure_for_endpoint`. Every failure must serialize a non-empty `hints` array; JSON and text render the same `Output`. Do not replace unknown-mutation reconciliation hints with generic retry advice.
8. **Auth metadata.** Derive catalog auth from the operation's supported API-key/OAuth security alternatives, and permissions from both OAuth security scope lists and `x-roblox-scopes`. Never infer OAuth-only behavior from an old survey. `tests/fixtures/openapi-auth-contract.json` retains official operation excerpts; refresh them deliberately and run `cargo test catalog_contract_tests` when changing catalog metadata. Cookies stay unsupported.

## Coding standards

- **Edition:** Rust 2024; `rust-version = "1.99"` in `Cargo.toml`.
- **Format / lint:** `cargo fmt`; `cargo clippy --all-targets -- -D warnings` (warnings are errors).
- **Tests:** Unit tests beside modules (`#[cfg(test)]`). Prefer testing path builders, parsers, and hint text without live API calls.
- **Dependencies:** Keep the dependency set minimal; no Roblox toolchain pins in `mise.toml` unless a task needs them.
- **Secrets:** Never log or commit credentials. Mask in summaries (`auth.rs` patterns). Do not add a home-directory `.env` fallback.

## Testing and CI

GitHub Actions (`.github/workflows/ci.yml`): `cargo fmt --check`, clippy, test, release build, catalog JSON smoke test, exit codes **2** (`auth_missing`) and **64** (unknown endpoint).

`src/http-contract-tests.rs` captures requests on loopback with synthetic credentials. `src/catalog-contract-tests.rs` and `scripts/check-coverage.mjs` discover actual CLI commands and check catalog coverage in both directions, not just entry counts.

When changing CLI behavior, run locally:

```bash
mise run verify
```

## Git and repo hygiene

- **Do not track:** `.rbxoc/`, `.ralph/`, `.pi/`, `target/`.
- **Do not commit** local agent transcripts, sweep scripts output, or `.env` from consumer projects.
- Release versioning lives in `Cargo.toml`; tag releases as `vX.Y.Z` on GitHub when publishing to crates.io.
- A published version bump also updates the DevForum announcement in the same release: https://devforum.roblox.com/t/rbxoc-a-cli-for-the-roblox-open-cloud-api/4959453 . The post's current version, Rokit pin, `.env` / `RBXOC_API_KEY` setup, and command screenshot must match the release. Do not leave the previous version or `.rbxoc/api-key` in that post.

## Boundaries (do not do without explicit ask)

- Force-push `main`, rewrite published tags, or publish to crates.io without maintainer intent.
- Call live Open Cloud with real credentials in CI or in committed scripts.
- Expand scope into unrelated Roblox tooling (Rojo, Studio automation) inside this crate.
- Duplicate long API documentation in AGENTS.md — link to `README.md` and `docs/` instead.

## Live API verification (maintainers)

Use a **real** universe ID and project-local `.env` / `.rbxoc` in a consumer repo (e.g. game template), not invented IDs. See `docs/live-api-verification.md` for known platform limits (PerformanceMonitoringV2 enrollment for alerts, place publish 409 busy). Analytics now advertises both API-key and OAuth support in the official contract; that does not establish live verification or account enrollment.
