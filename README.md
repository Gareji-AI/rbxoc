# rbxoc

<p align="center">
  <img src="https://raw.githubusercontent.com/Gareji-AI/rbxoc/main/assets/icon.png" alt="rbxoc icon" width="160" height="160">
</p>

[![CI](https://github.com/Gareji-AI/rbxoc/actions/workflows/ci.yml/badge.svg)](https://github.com/Gareji-AI/rbxoc/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/rbxoc.svg)](https://crates.io/crates/rbxoc)
[![Crates.io downloads](https://img.shields.io/crates/d/rbxoc.svg)](https://crates.io/crates/rbxoc)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A Rust CLI for the Roblox Open Cloud API, **designed for coding agents**.

Unofficial; not affiliated with or endorsed by Roblox.

Announcement: [rbxoc on the Roblox DevForum](https://devforum.roblox.com/t/rbxoc-a-cli-for-the-roblox-open-cloud-api/4959453).

## Setup

One Open Cloud API key per game. rbxoc reads it from that project's `.env`.

### 1. Install

[Rokit](https://github.com/rojo-rbx/rokit) installs a prebuilt binary. No Rust toolchain.

```toml
# rokit.toml
[tools]
rojo = "rojo-rbx/rojo@7.7.1"
rbxoc = "Gareji-AI/rbxoc@0.7.1"
```

```bash
rokit install
rbxoc --version
```

Rokit needs that exact version. Other install paths are under [Install](#install).

### 2. Create a key for this universe

Create the key in the [Creator Dashboard credentials](https://create.roblox.com/dashboard/credentials) page. Scope it to this universe. A key from another game will run that game's commands here.

### 3. Save the key in the project

Open `.env` in the project root with an editor and add one line:

```text
RBXOC_API_KEY=your-api-key
```

Add `.env` to `.gitignore`. Do not write the key with `echo` or another shell command. That line stays in shell history.

rbxoc loads the nearest `.env` the same way Asphalt loads `ASPHALT_API_KEY`. It walks up from the working directory, uses the first file, and does not replace a variable that is already set. A nested directory with its own `.env` does not inherit the parent's key. There is no key in the home directory.

### 4. Check

```bash
rbxoc auth check --universe-id 123456
```

The report names the file or variable that supplied the key. The value is masked (`abcd****`).

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
  "summary": "no credentials found. Set RBXOC_API_KEY, or add RBXOC_API_KEY to the project's .env (see `rbxoc auth check`)",
  "hints": ["Set RBXOC_API_KEY", "Run `rbxoc auth check` to inspect the credential state"],
  "error": {
    "code": "auth_missing",
    "message": "no credentials found. Set RBXOC_API_KEY, or add RBXOC_API_KEY to the project's .env (see `rbxoc auth check`)"
  }
}
```

`error.code` is a stable identifier. HTTP failures map to `http_401`, `http_403`, `http_5xx`, and so on. Agents branch on codes, not on prose.

### 3. Troubleshooting guidance

Every failure carries a non-empty `hints` array, including local input,
registry, file, and response-parsing errors. JSON and text output render the
same failure object. HTTP failures also receive status-specific next actions:

- `401` — reissue the API key, run `auth check`
- `403` — check permissions and scopes, note when OAuth2 is required
- `408` — the provider reported a timeout; it may still have processed the request
- `429` / `5xx` on replay-safe reads — bounded retry/backoff
- `mutation_outcome_unknown` — reconcile the target state or receipt before
  submitting again; never blindly repeat a mutation
- `read_outcome_unknown` — a read-only query failed transiently; nothing was
  written, so repeating the read is safe
- `unexpected_redirect` — Open Cloud answered with a 3xx; rbxoc refuses it

`--retry` defaults to `2` additional attempts, **only** for safe HTTP methods
(GET, HEAD, OPTIONS, TRACE) after transport errors, 429, or 5xx. Other methods
are sent at most once, including increments, messages, resource creation,
paid badge creation, restarts, and read-only POST queries. A cloneable request
body is not proof of idempotency. Uncertain mutation outcomes preserve the
HTTP status (when known) and return reconciliation guidance rather than
automatic retry advice. Asset sync retains its durable publication receipts
and recovery flow.

"Sent at most once" is a retry policy, not a claim that the request mutated
anything. A read-only query such as `analytics metrics` uses POST but writes
nothing, so its transient failures return `read_outcome_unknown` with retry
advice that is truthful. `POST`, `PATCH`, `PUT` and `DELETE` that receive HTTP
408 are treated like a lost response: the status is preserved, no automatic
replay happens, and the failure asks the caller to reconcile the target first.

Redirects are never followed. A `307`/`308` could replay a mutation and reqwest
would forward the `x-api-key` header to whatever origin the response points at,
so rbxoc returns `unexpected_redirect` instead of sending a second request.

### 4. Secret handling

An Open Cloud API key is scoped to one universe, so a single global key is the
wrong model: it makes it easy to run one game's commands against another
game's key. Credentials are resolved from the current project first.

Lookup order:

1. `--api-key` (discouraged; it lands in shell history)
2. `RBXOC_API_KEY` — already in the environment, or loaded from the nearest
   `.env` when that variable is unset
3. `--api-key-file <path>`
4. `RBXOC_OAUTH_TOKEN` (non-empty; used if no API key above was selected).
   The same `.env` load can set this when the process does not already have it.

rbxoc loads the nearest `.env` the same way Asphalt loads `ASPHALT_API_KEY`:
`dotenvy` walks up from the working directory, applies the first file, and
does not override a variable that is already set. A missing or unreadable
`.env` is ignored. Empty environment values are skipped. An explicitly
selected empty or unreadable key file is an error, not a reason to fall back
to another project's credential.

The project file is the `.env` from [Setup](#setup). There is no
home-directory fallback, so a key never leaks from one repository into another,
and a nested directory with its own `.env` never inherits its parent's key.

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

### Pin the version with Rokit (recommended for Roblox projects)

[Rokit](https://github.com/rojo-rbx/rokit) installs a prebuilt binary from the
GitHub Release, so there is no Rust toolchain and no native C build environment
to set up. Pin it next to `rojo`, `stylua`, and `selene` in the same file:

```toml
# rokit.toml
[tools]
rojo = "rojo-rbx/rojo@7.7.1"
rbxoc = "Gareji-AI/rbxoc@0.7.1"
```

```bash
rokit install
rbxoc --version
```

Rokit's `aftman.toml` and `foreman.toml` compatibility also works, so an
existing project does not have to migrate first. Rokit requires an **exact**
version; a requirement like `^0.6` is rejected.

`0.7.1` carries `windows-x86_64`, `linux-x86_64`, `macos-aarch64`, and
`macos-x86_64` archives, each with a `.sha256` sidecar. `macos-x86_64` was
added after `0.6.0`. **Rokit 1.2.0 does not read the sidecar.** Verify it
yourself if you need integrity beyond a pinned version, a GitHub Release, and
Rokit's own trust prompt:

```bash
sha256sum -c rbxoc-0.7.1-linux-x86_64.tar.gz.sha256
```

### From crates.io

```bash
cargo install rbxoc
```

This builds from source, so it needs Rust **1.99** or newer. It is the fallback
for architectures without a prebuilt archive, such as Linux arm64.

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

Put the key in the project `.env`, then verify it:

```bash
rbxoc auth check --universe-id 123456
```

The `.env` line is `RBXOC_API_KEY=your-api-key`. The check reports which file
or variable supplied the key, so a key meant for another game is obvious:

```json
{
  "ok": true,
  "summary": "Credentials work (api-key / abcd****)",
  "data": { "source": "/work/my-game/.env (RBXOC_API_KEY)" }
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

The place-version operation declares `x-roblox-size-limit` 10485760 (10 MiB), so
a `.rbxl` / `.rbxlx` file above that size fails locally as
`place_file_too_large` before any request is built. A file exactly at the limit
is still accepted.

### Rename a place or change its description

The Cloud v2 `Universe.displayName` and `Universe.description` are read-only:
the name and description consumers see come from the **root place**. Write them
through the place:

```bash
rbxoc place update --universe-id 123 --place-id 456 --display-name "Example Place"
rbxoc place update --universe-id 123 --place-id 456 --description-file description.txt
rbxoc place update --universe-id 123 --place-id 456 --server-size 20
rbxoc place update --universe-id 123 --place-id 456 --display-name "Example Place" --dry-run
```

`updateMask` contains exactly the flags you pass, so omitted fields keep their
current value; a call with no field flags fails before any cloud call.
`--dry-run` prints the planned mask and body, needs no credentials, and sends
nothing. Requires `universe.place:write`. A Team Create session returns HTTP 409.

### Assets

```bash
rbxoc asset create --file model.fbx --display-name "My Model" --user-id 789
rbxoc asset operation --operation-id ABC123
rbxoc asset update --asset-id 2205400862 --file v2.fbx
rbxoc asset update --asset-id 2205400862 --display-name "Renamed Model" --description "New description"
rbxoc asset update --asset-id 2205400862 --icon assets/123 --preview assets/456,assets/789 --dry-run
rbxoc asset rollback --asset-id 2205400862 --version 1
```

`asset update` replaces content only for Models. Omit `--file` for metadata-only
updates; metadata may also accompany a Model content update. `--icon` and
`--preview` take existing **Image** resources (`assets/123`), not local files
or Decal IDs. Repeat `--preview` or separate references with commas to replace
the preview list. Icons must be square. Dry-run validates the local plan without
credentials or requests; it cannot verify remote type, ownership, or dimensions.
Content-only updates omit `updateMask`; metadata masks select only supplied fields.
The official metadata-only examples omit `fileContent` even though the formal
multipart schema marks it required. These builders follow those examples and are
covered by offline wire tests, not live provider verification.

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

XML Animations at least **2 MiB** receive an advisory in `asset create --dry-run`
and `assets sync --dry-run`, also retained in sync success/failure hints. One
large XML Animation reportedly failed with HTTP 500 while its equivalent binary
export worked; the cause is unconfirmed. This threshold is **not a Roblox limit**
and smaller files are not guaranteed to upload. Consider exporting `.rbxm` in
Studio and checking poses before a new upload. rbxoc does not convert files or
change their fingerprints. Never replace the source of an uncertain publication
and retry: reconcile its pending receipt with the original bytes first.
Registry dry-run still does not perform full preprocessing/upload preflight.

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
rbxoc assets sync --update --path assets/models/tree.fbx --dry-run # plan a changed Model update
rbxoc assets sync --update --path assets/models/tree.fbx # keep its ID, record fresh receipt
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
files are **not** automatically replaced. Explicit `--update --path` updates
only changed, receipt-backed Models under their existing IDs; missing,
unverified, current, and unsupported entries fail before any cloud mutation.
Legacy/manual registrations stay unverified. See
[asset sync safety and recovery](docs/asset-sync-safety.md) for
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
rbxoc thumbnail delete --universe-id 123 --thumbnail-ids one,two
```

Deletion repeats the `homepageThumbnailIds` query parameter once per ID;
empty IDs fail locally. No JSON body is sent. The documented 200 body maps each
requested ID to a boolean, so `data.deleted`, `data.notDeleted` and
`data.unconfirmed` report the provider's per-ID result. A `false`, absent or
non-boolean entry is never counted as deleted: a mixed result keeps
`ok: true` with `data.partial` and `"the result is partial"` in the summary,
and a result with no confirmed deletion fails as `thumbnail_delete_failed`.

### Server logs

```bash
rbxoc server list --universe-id 123 --place-id 456 --version 12
rbxoc server logs --universe-id 123 --place-id 456 --version 12 --job-id <jobId> --limit 100
rbxoc server restart-forecast --universe-id 123
rbxoc server restart --universe-id 123 --dry-run
```

`server restart --dry-run` calls the same read-only GET forecast endpoint as
`restart-forecast`; it never sends a restart mutation. Without `--dry-run`,
`server restart` requests an actual restart and is sent at most once. The
restart body is `{}`. Config restore and asset archive declare no body, so
those posts send no JSON object.

`--limit` is `MaxPageSize` (1–100), not a client-side trim. `0` or `--full`
follows `nextPageToken` with `MaxPageSize=100`. A later page uses
`--page-token`. `server list` uses the query name `PageToken`.

`server list` exposes the documented partial-result signals. When the provider
sets `activeServersFetchError` or `shutdownServersFetchError`, the summary says
the result is partial and `data.partial` / `data.missingSources` name which half
of the combined stream is missing. The returned `gameServers`, `jobIds`,
`totalCount` and page tokens stay usable for reconciliation.

### Standard DataStores

```bash
rbxoc datastore set --universe-id 123 --store players --entry player_42 \
  --value '{"coins":100}' --create
```

Values retain their JSON types: objects, arrays, strings, numbers, booleans,
and null are not converted to strings. Quote JSON strings explicitly, for
example `--value '"hello"'`. A value update reads the entry first, so the key also needs
`universe-datastores.objects:read`, and then sends
`value`, `attributes`, `users`, and `etag` together. A forbidden read returns
`datastore_read_forbidden` and does not PATCH. Omitting attributes or
users on a PATCH would clear them, so a missing etag fails locally with
`etag_missing` and sends no PATCH. Pass `--attributes` or `--users` to replace
them, or `--clear-attributes` and `--clear-users` to store `{}` and `[]`.
A missing entry without `--create` fails with `entry_not_found` and sends no
PATCH. `--create` allows creation when missing and is not a create-only guard
against overwriting an existing entry. Standard DataStores send
`allowMissing=true`; ordered DataStores send `allow_missing=true`. Without
`--create`, the parameter is omitted. List pages accept `--page-token`
(`pageToken` for standard stores, `page_token` for ordered stores).

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
rbxoc config get --universe-id 123 --repository InExperienceConfig
rbxoc config patch-draft --universe-id 123 --repository InExperienceConfig \
  --document '{"entries":{"newShop":true}}'
rbxoc config publish --universe-id 123 --repository InExperienceConfig \
  --draft-hash <draftHash> --deployment-strategy Immediate
rbxoc config revisions --universe-id 123 --repository InExperienceConfig
```

Repository names are the public enum (`InExperienceConfig`,
`RecommendationServicesConfig`, `DataStoresConfig`, `ExtendedServicesConfig`,
`LeaderboardsConfig`, `ExperienceUserConfig`, `JourneysConfig`,
`AntiCheatConfig`). Draft documents accept only `draftHash`, `entries`, and
`conditionalRules`. Invalid names and unknown fields fail before authentication.
Editing a draft never changes live values. `config publish` requires the
reviewed `--draft-hash`, and may include `--message` and
`--deployment-strategy Immediate` or `GradualRollout`.

### Analytics

```bash
rbxoc analytics catalog
rbxoc analytics metrics --universe-id 123 --query '{"metric":"DailyActiveUsers","granularity":"OneDay","startTime":"2026-10-01T00:00:00Z","endTime":"2026-10-08T00:00:00Z"}'
rbxoc analytics dimensions --universe-id 123 --query '{"metric":"DailyActiveUsers","dimensions":["Country"],"startTime":"2026-10-01T00:00:00Z","endTime":"2026-10-08T00:00:00Z"}'
rbxoc analytics alerts list --universe-id 123
rbxoc analytics alerts incidents --universe-id 123 --start-time 2026-10-01T00:00:00Z --end-time 2026-10-08T00:00:00Z
```

`QueryRequest` requires one `metric` plus `granularity`, `startTime`, and
`endTime`. Granularity is `OneMinute`, `HalfHour`, `OneHour`, `OneDay`,
`OneWeek`, `OneMonth`, or `None`. A closed-enum miss fails locally before
authentication.

`DimensionValuesRequest` requires `metric`, a non-empty `dimensions` array,
`startTime`, and `endTime`. It takes the same optional `granularity`, `filter`,
and `limit`, and never computes the metric itself. Read the returned values
back as `breakdown` and `filter` inputs for `analytics metrics`.

The current contract supports API keys and OAuth2 for analytics queries and
operation reads. Both need the documented `universe.analytics:read` permission.
See `rbxoc api show analytics.metrics`; contract support is not evidence that a
particular account/key is enrolled or that a query has been live-verified.

Alert commands use the provider's case-sensitive `Universe` resource type
automatically, for example `/analytics-alert-control-plane/v1/resource/Universe/id/123/alerts`.

### Monetization, badges, and events

```bash
rbxoc gamepass list --universe-id 123
rbxoc gamepass create --universe-id 123 --name "VIP" --price 499 --image vip.png
rbxoc gamepass update --universe-id 123 --game-pass-id 456 --price 499

rbxoc badge create --universe-id 123 --name "Wave 10" --description "Reach wave 10" \
  --payment-source user --expected-cost 100 --active true --icon badge.png
rbxoc badge update --badge-id 456 --description "Reach wave 10"

rbxoc event list --universe-id 123
rbxoc event create --universe-id 123 --document '{"title":"Winter","startTime":"2026-12-01T00:00:00Z"}'
```

`badge create` spends Robux. It requires `--payment-source`, `--expected-cost`,
and `--active true|false` before it builds a request. Only `user` (provider
enum `1`) and `group` (`2`) are accepted. The multipart body sends `name`,
`description`, numeric `paymentSourceType`, numeric `expectedCost`, and
`isActive`. `--icon` is optional and, when present, is validated locally as a
nonempty `.png`, `.jpg`, or `.jpeg` file and sent as the `files` part. Omit
`--icon` only when the universe already has a curated default badge icon.
Missing or invalid confirmation sends nothing.
`gamepass create --price` also sends `isForSale=true`. `gamepass update --price`
changes only the price; sale state changes only with `--is-for-sale true` or
`--is-for-sale false`.

Alert and event bodies are provider-defined JSON, passed through
verbatim rather than re-declared here. `universe publish-message` sends the
exact string in `{"message":"..."}`. A JSON string such as `"hello"` is
unwrapped once; object-shaped text is not parsed into a new document. The
limit is 1,024 characters.

### Asset permissions

```bash
rbxoc permissions grant --asset-id 111 --asset-id 222 \
  --subject-type group --subject-id 999 --action use
rbxoc permissions grant --asset-id 111 --subject-type all --action use
```

`--subject-type` is `user`, `group`, `group-roleset`, `universe`, or `all`.
`--action` is `use`, `edit`, or `download`. `all` sends an empty `subjectId`
and rejects `--subject-id`. Every other subject type requires a positive ID.

HTTP 200 can still contain per-asset grant failures. `permission_grant_failed`
preserves `successAssetIds` and the original `response.errors`; a missing or
malformed completion receipt returns `invalid_permissions_response`. Only a
complete confirmed batch reports success. Inspect existing permissions before
retrying individual failures, never blindly replay the whole batch.

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

Authentication is operation-specific. The 2026-10-17 contract supports both
API keys and OAuth2 for analytics, server reads, messaging, assets, configs,
events, badges and monetization. Thumbnail, asset-permission, standard/ordered
DataStore, place-publish and place-history operations advertise API keys only
after excluding unsupported cookies. Check `auth` and `scopes` in
`rbxoc api show <id>` rather than reusing another endpoint's permission names.

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

Tool updates therefore stay inside the project and arrive as Renovate pull
requests (`renovate.json`): the `rust` pin and `cargo:similarity-rs` in
`mise.toml`, plus the Cargo dependency graph and GitHub Actions in CI.

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

### Optional live sweep (maintainers only)

Both `node scripts/live-sweep.mjs` and `node scripts/live-sweep-ext.mjs` use
one shared case list. They default to **read-only**, with no committed target
defaults. Select a small read-only probe with explicit environment values:

```bash
UNIVERSE_ID="$MY_TEST_UNIVERSE" RBXOC_SWEEP_CASES="universe get" node scripts/live-sweep.mjs
```

`RBXOC` can select a binary. Without it, the platform-specific release binary
in this checkout is used. `RBXOC_SWEEP_CASES` is a comma-separated list of
exact labels from `scripts/live-sweep-cases.mjs`. Without a selection, all
read-only cases run, requiring every target they use (universe/place/asset IDs,
server version/job ID, config repository, datastore name/scope/entry, and
`ANALYTICS_QUERY`). Missing configuration aborts the **whole** plan before
any child command. The script prints mode and targets before starting.

Mutations require **exactly** `RBXOC_SWEEP_MUTATE=1` plus explicit targets,
files, message/value bodies, and prices for selected cases. Selecting a
mutating case without opt-in fails, rather than silently running it. Prefer
a disposable test universe and a narrowly selected case; mutations can publish
places, overwrite/delete data, grant permissions, send messages, and change
game pass prices. No cloud rollback is promised. The obsolete unsupported
`devproduct update` case was removed. Nonzero child exits fail the sweep.

### Recorded evidence

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
| Asset uploads/updates, place publishing, real server logs, analytics with either credential family, paid product creation | Live behavior remains unverified |

Offline fixtures test publication safety and recovery, including stable-ID
Model updates; they do **not** establish live moderation, latest-version
delivery, or Roblox platform guarantees. CI runs the coverage checker after
the release build. Use `rbxoc api list` for current endpoint counts rather
than interpreting historical report counts as current coverage.

## License

Copyright (c) 2026 eiei114.

Licensed under either [MIT](LICENSE) or [Apache-2.0](license-apache), at your
option. Unless explicitly stated otherwise, contributions are made under the
same dual license.
