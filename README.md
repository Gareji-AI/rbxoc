# rbxoc

[![CI](https://github.com/eiei114/rbxoc/actions/workflows/ci.yml/badge.svg)](https://github.com/eiei114/rbxoc/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A Rust CLI for the Roblox Open Cloud API, **designed for coding agents**.

## Why this exists

The existing [`rbxcloud`](https://github.com/Sleitnick/rbxcloud) (139 stars) is a solid CLI, but its last commit was 2025-03-23 and it does not cover the APIs added since.

| Area | rbxcloud | rbxoc |
| --- | --- | --- |
| Thumbnail replacement | ✗ | ✅ |
| Game server logs | ✗ | ✅ |
| Analytics queries | ✗ | ✅ |
| Place version history | ✗ | ✅ |
| Place publishing | ✅ | ✅ |
| Asset management | ✅ | ✅ |
| DataStores | ✅ | ✅ |
| Monetization | ✗ | ✅ |

Most of the "Best ... 2026" repositories that rank highly in GitHub search are empty shells with inflated star counts. Real options are limited.

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
  "command": "auth",
  "summary": "no credentials found. Set RBXOC_API_KEY, or put an API key in ~/.rbxoc/api-key",
  "hints": ["Set RBXOC_API_KEY", "Run `rbxoc auth check` to inspect the credential state"],
  "error": { "code": "auth_missing", "message": "..." }
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

Environment variables and files take priority over arguments. `--api-key` lands in shell history, so agents should avoid it.

Lookup order:

1. `--api-key` (discouraged)
2. `RBXOC_API_KEY` environment variable
3. `--api-key-file <path>`
4. `~/.rbxoc/api-key`
5. `RBXOC_OAUTH_TOKEN` / `~/.rbxoc/oauth-token`

Output always masks the value (`abcd****`).

### 5. Exit codes

| code | meaning |
| --- | --- |
| 0 | success |
| 1 | runtime error (HTTP failure and similar) |
| 2 | no credentials found |
| 64 | bad arguments (unknown endpoint ID and similar) |

## Install

### mise (recommended)

```bash
mise install            # Rust / Rojo / pesde
mise run build          # release binary
mise run install-local  # into ~/.local/bin
```

### cargo

```bash
cargo install --path . --locked
```

## Usage

### Check credentials

```bash
export RBXOC_API_KEY="..."
rbxoc auth check --universe-id 123456
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

Supported formats (20 MB per request): `.fbx` `.gltf` `.glb` `.rbxm` `.rbxmx` (Model), `.png` `.jpeg` `.bmp` `.tga` (Image), `.mp3` `.ogg` `.wav` `.flac`, `.mp4` `.mov`.

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
```

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

## Authentication coverage

Open Cloud has three auth families. This CLI excludes cookies.

| Method | Supported | Use |
| --- | --- | --- |
| `x-api-key` | ✅ | most endpoints |
| OAuth2 bearer | ✅ | Analytics, alerts, and similar |
| legacy cookie | ✗ | browser-session derived, unsuited to automation |

**Note**: some areas are OAuth2-only. Analytics queries need `universe.analytics:read` and cannot be called with an API key. Check the `auth` field of `rbxoc api show <id>`.

## Toolchain

`mise.toml` pins Rojo / pesde / Rust.

```bash
mise run check         # fmt / clippy / test
mise run build         # release build
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
mise run check   # fmt --check / clippy -D warnings / test
```

Tests never call the real API. They cover path building, extension inference, response-shape extraction, backoff math, and catalog consistency.

## Not verified

This repository **has not been exercised against the live API with a real key**. Only the implementation, compilation, and unit tests that avoid the network exist. Actual responses per endpoint are unverified.

Specifically unverified:

- Place publishing with a real file
- Asset upload processing
- The thumbnail API (experimental, so the shape may change)
- Analytics (requires OAuth2)
- Fetching real game server logs

## License

MIT
