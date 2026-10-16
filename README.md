# rbxoc

[![CI](https://github.com/eiei114/rbxoc/actions/workflows/ci.yml/badge.svg)](https://github.com/eiei114/rbxoc/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/rbxoc.svg)](https://crates.io/crates/rbxoc)
[![Crates.io downloads](https://img.shields.io/crates/d/rbxoc.svg)](https://crates.io/crates/rbxoc)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A Rust CLI for the Roblox Open Cloud API, **designed for coding agents**.

## Why this exists

The existing [`rbxcloud`](https://github.com/Sleitnick/rbxcloud) (139 stars) is a solid CLI, but its last commit was 2025-03-23 and it does not cover the APIs added since.

| Area | rbxcloud | rbxoc |
| --- | --- | --- |
| Thumbnail replacement | ✗ | ✅ |
| Game server logs | ✗ | ✅ |
| Analytics queries | ✗ | ✅ |
| Analytics alerts | ✗ | ✅ |
| Place version history | ✗ | ✅ |
| Remote config | ✗ | ✅ |
| Ordered DataStores (leaderboards) | ✗ | ✅ |
| Virtual events | ✗ | ✅ |
| Asset permissions | ✗ | ✅ |
| Badges | ✗ | ✅ |
| Asset ID codegen (Luau / TypeScript) | ✗ | ✅ |
| Monetization (Game Pass / Dev Product) | ✗ | ✅ |
| Place publishing | ✅ | ✅ |
| Asset management | ✅ | ✅ |
| DataStores | ✅ | ✅ |

Most of the "Best ... 2026" repositories that rank highly in GitHub search are empty shells with inflated star counts. Real options are limited.

Full coverage map: [docs/open-cloud-coverage.md](docs/open-cloud-coverage.md).

## Agent-facing design

### 1. Deterministic discovery

`rbxoc api list` returns a self-describing catalog. An agent learns what is possible without hunting through source.

```bash
rbxoc api list --implemented
rbxoc api list --query thumbnail
rbxoc api show place.publish
rbxoc api mutating          # mutating operations only, for pre-flight review
```

`src/catalog.rs` is the source of truth and maps 1:1 to the implementation. Unimplemented entries still appear with `implemented: false` and a reason, so agents do not waste attempts on features that do not exist.

### 2. Machine-readable output

Every command returns the same envelope. `--format json` is the default.

```json
{
  "ok": true,
  "command": "place publish",
  "summary": "Place published (version 12)",
  "data": { "universeId": 1, "placeId": 2, "version": 12 },
  "hints": ["Run `rbxoc place history --place-id <id>` to inspect the history"]
}
```

On failure:

```json
{
  "ok": false,
  "command": "auth check",
  "summary": "no credentials found. Set RBXOC_API_KEY, or create .rbxoc/api-key in this project (see `rbxoc auth check`)",
  "hints": ["Set RBXOC_API_KEY", "Run `rbxoc auth check` to inspect the credential state"],
  "error": {
    "code": "auth_missing",
    "message": "no credentials found. Set RBXOC_API_KEY, or create .rbxoc/api-key in this project (see `rbxoc auth check`)"
  }
}
```

`error.code` is a stable identifier. HTTP failures map to `http_401`, `http_403`, `http_5xx`, and so on. Agents branch on codes, not on prose.

### 3. Troubleshooting guidance

Failures always carry `hints`. Each HTTP status maps to concrete next actions:

- `401` — reissue the API key, run `auth check`
- `403` — check permissions and scopes, note when OAuth2 is required
- `429` — exponential backoff, mention `--retry`
- `5xx` — transient failure, retry later

### 4. Secret handling

An Open Cloud API key is scoped to one universe, so a single global key is the
wrong model: it makes it easy to run one game's commands against another
game's key. Credentials are resolved from the current project first.

Lookup order:

1. `--api-key` (discouraged; it lands in shell history)
2. `RBXOC_API_KEY` environment variable
3. `--api-key-file <path>`
4. `RBXOC_OAUTH_TOKEN` (non-empty; used if no API key above was selected)
5. `.rbxoc/api-key`, walking up from the working directory and stopping at the
   first `.rbxoc` found
6. `.rbxoc/oauth-token` in that same first directory, if it has no API key

Empty environment values are skipped. An explicitly selected empty or unreadable
key file is an error, not a reason to fall back to another project's credential.

Place the key beside the project:

```bash
echo "your-api-key" > .rbxoc/api-key
```

Add `.rbxoc/` to the project's `.gitignore`. There is no home-directory
fallback, so a key never leaks from one repository into another, and a nested
project with its own `.rbxoc` never inherits its parent's key.

`rbxoc auth check` reports the exact selected file path, environment variable,
or `--api-key` from the credential resolver itself, so a wrong-project key is
immediate to spot. Output always masks the value (`abcd****`).

### 5. Exit codes

| code | meaning |
| --- | --- |
| 0 | success |
| 1 | runtime error (HTTP failure and similar) |
| 2 | no credentials found |
| 64 | bad arguments (unknown endpoint ID and similar) |
| 70 | internal output serialization or help/version output failure |

Unknown subcommands, missing arguments, invalid enum values, and invalid numeric
arguments return `error.code = "usage_error"`, exit 64, and the standard JSON
envelope on **stdout**, even with `--format text`. Diagnostics are in
`error.detail`; stderr is not the machine-readable surface. `--help` and
`--version` retain normal human-readable output and exit 0.

## Install

### From crates.io

```bash
cargo install rbxoc
```

This is the quickest path. It builds the released version from source.

### From a checkout

```bash
cargo install --path . --locked
```

### mise (for development)

```bash
mise install            # Rust toolchain
mise run build          # release binary
mise run install-local  # into ~/.local/bin
```

## Usage

### Check credentials

Put the key beside the project, then verify it:

```bash
mkdir -p .rbxoc
echo "your-api-key" > .rbxoc/api-key
rbxoc auth check --universe-id 123456
```

The check reports which file or variable supplied the key, so a key meant for
another game is obvious:

```json
{
  "ok": true,
  "summary": "Credentials work (api-key / abcd****)",
  "data": { "source": "/work/my-game/.rbxoc" }
}
```

`RBXOC_API_KEY` also works for one-off runs and CI, where a file may not be
convenient:

```bash
RBXOC_API_KEY="..." rbxoc auth check --universe-id 123456
```

### Publish a place

```bash
rbxoc place publish --universe-id 123 --place-id 456 --file build/game.rbxlx
rbxoc place publish --universe-id 123 --place-id 456 --file build/game.rbxlx --published
rbxoc place history --place-id 456
```

### Assets

```bash
rbxoc asset create --file model.fbx --display-name "My Model" --user-id 789
rbxoc asset operation --operation-id ABC123
rbxoc asset update --asset-id 2205400862 --file v2.fbx
rbxoc asset rollback --asset-id 2205400862 --version 1
```

Supported formats (20 MB per request): `.fbx` `.gltf` `.glb` `.rbxm` `.rbxmx` (Model), `.png` `.jpeg` `.bmp` `.tga` (Decal image uploads), `.mp3` `.ogg` `.wav` `.flac`, `.mp4` `.mov`.

**Decal ID is not Image ID.** An image upload's completed `assetId` remains
the original Decal ID; rbxoc never silently redefines it. For ImageLabel or
ImageButton, resolve the existing Decal's `Texture` in Studio, then use the
explicit offline workflow:

```bash
rbxoc asset image-id --decal-id 123 --texture rbxassetid://456
rbxoc assets register --path assets/ui/icon.png --id 123 --image-id 456
```

Enable `[codegen] image_ids = true` for generated image-consumer references.
Default codegen keeps original IDs for compatibility. Without `--texture`,
`asset image-id` returns an actionable unresolved error, not a fabricated ID
or another upload. See [Image IDs and Studio fallback](docs/image-ids.md).

### 3D models and animations

`.rbxm` and `.rbxmx` map to two different asset types that share the same
extension and the same `model/x-rbxm` content type:

| Asset type | Root class | Note |
| --- | --- | --- |
| `Animation` | `KeyframeSequence` or `CurveAnimation` | content is not updatable |
| `Model` | anything else | content is updatable |

The extension cannot tell them apart, so `rbxoc` parses the file with the same
DOM readers Rojo uses and reports what it found.

```bash
rbxoc asset create --file walk.rbxmx --display-name "Walk" --user-id 789 --dry-run
```

```json
{
  "assetType": "Animation",
  "contentType": "model/x-rbxm",
  "resolutionNote": "walk.rbxmx has class KeyframeSequence, so it uploads as Animation"
}
```

`--dry-run` resolves the type without uploading and needs no credentials, so
the classification can be checked before spending an upload. Pass
`--asset-type` to override the inspection.

A malformed model file fails with `model_parse_failed` instead of guessing, so
a broken export does not silently land as the wrong asset type.

For 3D models, `.fbx` / `.gltf` / `.glb` upload as `Model` containing
MeshParts. Roblox does not expose importer settings through Open Cloud, so
[Studio's 3D Importer](https://create.roblox.com/docs/art/modeling/3d-importer)
remains the practical route when those settings matter.

### Asset ID registry and code generation

Keep asset IDs out of game code. `rbxoc assets` maintains a manifest and a
lockfile, then generates Luau or TypeScript that references assets by name.

```bash
rbxoc assets init --creator user:12345
rbxoc assets status
rbxoc assets register --path assets/ui/button.png --id 123456789
rbxoc assets codegen
rbxoc assets sync              # upload missing, refresh lock, codegen
rbxoc assets sync --path assets/ui/button.png --path assets/sfx/hit.ogg
rbxoc assets sync --dry-run     # registry + known-freshness check, not upload preflight
rbxoc assets import-asphalt-lock --asphalt-lock asphalt.lock.toml --input Assets --write
```

`assets sync` rasterizes `.svg` to PNG before upload. Import Asphalt v2 hash
locks with `import-asphalt-lock` when migrating from Asphalt.

Sync validates the entire selected batch before any upload, records each
successful ID immediately, and resumes known pending operations instead of
creating duplicates. `--path` selects exact manifest/lock keys, after
`path_key_from` normalization. Unknown keys fail; they never trigger full sync.
Codegen keeps all registered IDs and omits unselected missing files. Ambiguous
generated keys fail without overwriting existing output.

`assets status` distinguishes `current`, `changed`, and `unverified` entries.
Only a successful sync receipt proves known freshness. Changed registered
files are **not** automatically replaced, and legacy/manual registrations stay
unverified. See [asset sync safety and recovery](docs/asset-sync-safety.md) for
dry-run limits, pending operations, and uncertain outcomes.

Generated Luau from that registry:

```lua
-- This file is automatically @generated by rbxoc.
-- It is not intended for manual editing.
local Assets = {
	assets = {
		ui = {
			["button.png"] = "rbxassetid://123456789",
		},
	},
}

return Assets
```

Game code then writes `Assets.assets.ui["button.png"]` instead of a bare
number, and a re-upload only changes the lockfile.

Set `codegen.language = "typescript"` and `codegen.style = "flat"` for a
typed declaration instead:

```ts
// This file is automatically @generated by rbxoc.
// It is not intended for manual editing.
declare const Assets: {
	"assets/ui/button.png": string
}

export = Assets
```

`--check` fails when any manifest path lacks an ID, so CI can gate on a stale
registry:

```bash
rbxoc assets codegen --check
```

The design follows Tarmac and [Asphalt](https://github.com/jacktabscode/asphalt):
the manifest is hand-written, the lockfile is generated and committed, and the
source file is generated. Output is deterministic, so the generated file stays
diff-friendly.

### Thumbnails

```bash
rbxoc thumbnail upload --universe-id 123 --files a.png b.png
rbxoc thumbnail status --universe-id 123 --operation-ids ABC123
rbxoc thumbnail list --universe-id 123
```

### Server logs

```bash
rbxoc server list --universe-id 123 --place-id 456 --version 12
rbxoc server logs --universe-id 123 --place-id 456 --version 12 --job-id <jobId> --limit 200
```

### Leaderboards (Ordered DataStores)

```bash
rbxoc ordered-datastore set --universe-id 123 --store highscore --entry player_42 \
  --value 9500 --create
rbxoc ordered-datastore increment --universe-id 123 --store highscore --entry player_42 \
  --amount 500
rbxoc ordered-datastore list --universe-id 123 --store highscore --page-size 10
```

Values are integers, and `--scope` defaults to `global`. Unlike standard
DataStores, the scope segment is required, so it is always sent.

Named URL segments (store, scope, entry, config repository/revision, topic,
job, alert, and operation IDs) accept raw UTF-8. rbxoc percent-encodes each
segment independently: `/`, `?`, `#`, spaces, `%`, and non-ASCII names cannot
change the route. Do not pre-encode names (`%2F` means a literal percent-based
name, not a slash). Empty names and the standalone names `.` and `..` fail
locally with `invalid_path_segment` because URL parsers normalize dot segments.
Roblox can still impose its own endpoint-specific naming constraints.

### Remote config and feature flags

```bash
rbxoc config get --universe-id 123 --repository game
rbxoc config patch-draft --universe-id 123 --repository game \
  --document '{"flags":{"newShop":true}}'
rbxoc config publish --universe-id 123 --repository game
rbxoc config revisions --universe-id 123 --repository game
```

Editing a draft never changes live values. Only `config publish` does, and the
draft commands say so in their `hints`.

### Analytics

```bash
rbxoc analytics catalog
rbxoc analytics metrics --universe-id 123 --query '{"metrics":[{"metric":"DailyActiveUsers"}],"startTime":"2026-10-01T00:00:00Z","endTime":"2026-10-08T00:00:00Z","granularity":"P1D"}'
rbxoc analytics alerts list --universe-id 123
rbxoc analytics alerts incidents --universe-id 123 --start-time 2026-10-01T00:00:00Z --end-time 2026-10-08T00:00:00Z
```

This API needs OAuth2 (`universe.analytics:read`); an API key cannot call it.

### Monetization, badges, and events

```bash
rbxoc gamepass list --universe-id 123
rbxoc gamepass create --universe-id 123 --name "VIP" --price 499 --image vip.png

rbxoc badge update --badge-id 456 --description "Reach wave 10"

rbxoc event list --universe-id 123
rbxoc event create --universe-id 123 --document '{"title":"Winter","startTime":"2026-12-01T00:00:00Z"}'
```

`badge create` spends Robux, so it requires `--payment-source` and says so in
its result. Alert and event bodies are provider-defined JSON, passed through
verbatim rather than re-declared here.

### Asset permissions

```bash
rbxoc permissions grant --asset-id 111 --asset-id 222 \
  --subject-type group --subject-id 999 --action use
```

### Discover everything

```bash
rbxoc api list --implemented
rbxoc api mutating
```

See [docs/open-cloud-coverage.md](docs/open-cloud-coverage.md) for the full
coverage map and what is deliberately out of scope.

## Authentication coverage

Open Cloud has three auth families. This CLI excludes cookies.

| Method | Supported | Use |
| --- | --- | --- |
| `x-api-key` | ✅ | most endpoints |
| OAuth2 bearer | ✅ | Analytics, alerts, and similar |
| legacy cookie | ✗ | browser-session derived, unsuited to automation |

**Note**: some areas are OAuth2-only. Analytics queries need `universe.analytics:read` and cannot be called with an API key. Check the `auth` field of `rbxoc api show <id>`.

## Toolchain

`mise.toml` pins the Rust toolchain. That is the only external tool this CLI
needs; it calls the Open Cloud HTTP API and parses `.rbxm` / `.rbxmx` through
crates, so no Roblox tooling is pinned.

```bash
mise run check         # fmt / clippy / test
mise run build         # release build
mise run verify        # check, plus a release build and the coverage check
mise run demo          # catalog output that needs no credentials
```

### About automatic updates

mise's `self_update.auto` and `tool_update.global_auto` are **global-only settings**. A project `mise.toml` cannot enable them; mise rejects them for security reasons.

```text
mise WARN  self_update.auto in non-global config .../mise.toml is ignored for security reasons
```

Tool updates therefore stay inside the project:

```bash
mise run update-tools            # check for newer versions
mise run update-tools -- --apply # rewrite and run mise install
```

`mise latest <tool>` resolves the stable version. (`ls-remote` also returns aliases like nightly / beta / stable, so it cannot be used alone.)

## Development

```bash
mise run check    # fmt --check / clippy -D warnings / test
mise run verify   # check, plus a release build and the coverage check
node scripts/check-coverage.mjs
```

Tests never call the real API. They cover path building, extension inference,
response-shape extraction, backoff math, and catalog consistency.

The coverage checker asserts that the catalog and the binary agree: unique
IDs, a path and summary for every implemented entry, a stated reason for every
unimplemented one, and no cookie-only endpoint marked implemented.

## Verification status

A real-key session on **2026-10-13** verified credential resolution and the
universe read and virtual-event list endpoints. Real-key / fake-key / no-key
control requests returned 200 / 401 / 403 respectively. See the
[live verification report](docs/live-api-verification.md) for the recorded
results and limits; a 400/404 response is not proof of a successful operation.

| Area | Evidence |
| --- | --- |
| Auth probe, universe read, event list | Live-verified with the session's real API key |
| Registry, codegen, type inference, catalog | Offline-verified; no cloud call required |
| Thumbnails, server management, game passes, developer products, standard/ordered DataStores | Implemented; the session reached 401/403 because its key lacked scopes, not successful operation verification |
| Asset uploads/updates, place publishing, real server logs, OAuth2 analytics, paid product creation | Live behavior remains unverified |

Offline fixtures test publication safety and recovery, including stable-ID
Model updates; they do **not** establish live moderation, latest-version
delivery, or Roblox platform guarantees. CI runs the coverage checker after
the release build. Use `rbxoc api list` for current endpoint counts rather
than interpreting historical report counts as current coverage.

## License

MIT
