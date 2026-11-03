# Changelog

## 0.8.0

- `.env` credential lookup is bounded by the project root (the nearest ancestor
  holding `.git`, `.rbxoc`, `rokit.toml`, or `rbxoc.assets.toml`). A `.env`
  above that boundary, or in the home directory, is no longer selected, so a
  universe-scoped key cannot arrive from a parent workspace. A project-local
  `.env` still works from nested directories, and an existing `RBXOC_API_KEY`
  still wins.
- Post-send response validation no longer emits "correct the input" or "before
  trying again" guidance. `resource_id_missing`, `version_number_missing`,
  `invalid_asset_response`, `invalid_asset_version`, and the other receipt codes
  now share one classification in `ctx::is_post_send_validation` and return
  reconciliation-only hints: the mutation was sent at most once, inspect the
  target before resubmitting. Pre-request validation keeps its input hints.
- `datastore set` and `datastore delete` fail closed when the entry value holds a
  profile session lock (`value.MetaData.ActiveSession` for ProfileStore /
  ProfileService, or a top-level `value.SessionJob` string): `entry_session_locked`
  and no write. `--ignore-session-lock` is the escape hatch for dead sessions.
  `datastore delete` now reads the entry first and sends the documented `etag`
  as `If-Match`. `datastore get` / `datastore list` report the same lock as a
  derived `sessionLock` flag (`null` when no recognized shape matches).
- `usage_error` envelopes name the leaf path clap matched (`datastore set`)
  instead of the placeholder `usage`, so hints no longer point at the
  nonexistent `rbxoc usage` command. `usage` survives only when nothing matched,
  and no command-specific hint is emitted then.
- `asset create --asset-type Animation` accepts `.rbxm` / `.rbxmx` as
  `model/x-rbxm`, matching what the inference path already sent. Unmapped
  combinations still fail locally.
- Assets API request JSON preserves documented integer types:
  `creationContext.creator.userId` / `groupId` and the update `assetId` are JSON
  numbers, while `icon` and `previews[].asset` stay resource-path strings. IDs
  above the signed int64 range fail locally before any mutation is sent.
- Dispatch compares the resolved credential family with the target catalog
  entry and rejects an unsupported one locally with `auth_kind_unsupported`
  before any request. 401/403 hints now describe the credential that was sent,
  so an OAuth token is never called an API key and a 403 never suggests switching
  to the family already in use. Dual-auth endpoints still accept either.
- `analytics catalog` runs without credentials, like the other local discovery
  commands. Cloud-backed analytics commands still require them.
- Analytics preflight matches the official schemas: `analytics dimensions` now
  accepts a query that omits `dimensions` (and an empty array, which has no
  `minItems`), still rejects non-string entries, and a present non-string
  `granularity` fails locally with `invalid_query` for both query commands.
- `permissions grant` supports `CopyFromRcc` and `UpdateFromRcc` through
  `--action copy-from-rcc` / `update-from-rcc` (the exact schema spelling also
  works). They serve `Place` assets granted to a `Universe`, enabling
  `AssetService:CreatePlaceAsync()` and `AssetService:UpdatePlaceAsync()`.
  `Invalid` and unknown values still fail locally and send nothing.
- `event create` validates the fields the virtual-events service requires, so an
  incomplete document fails locally with `invalid_document` and sends nothing.
  `title` and `subtitle` must be non-empty, `startTime` and `endTime` are
  required, and `visibility` must be `public` or `private` (`moderated` is
  server-managed). The README example is complete and a contract test keeps it
  that way.
- The README Rokit pins, archive list, and checksum example name the current
  release, and a release/version consistency test fails when `Cargo.toml`, the
  documented pin, and those examples drift apart.

## 0.7.2

- `analytics dimensions` queries the documented `dimension-values` endpoint, so
  `breakdown` and `filter` inputs can be read back instead of guessed. It takes
  the same `--query` shape as metrics minus `granularity`, rejects unknown
  fields, an empty or non-array `dimensions`, and a closed-enum granularity miss
  before authentication, and emits a `--kind dimensions` poll hint. The catalog
  entry is now `implemented: true` with API-key and OAuth2 alternatives from the
  official OpenAPI document.

## 0.7.1

- API keys are loaded the same way Asphalt loads `ASPHALT_API_KEY`. dotenvy
  reads the nearest `.env` before argument parsing and does not override an
  existing environment variable. The variable is `RBXOC_API_KEY`.
  `.rbxoc/api-key` and `.rbxoc/oauth-token` are no longer consulted.
- The README setup walks through install, a universe API key, and the project
  `.env`. The DevForum announcement is linked from the README and the crate
  homepage.

## 0.7.0

- Endpoint failures name the catalog permission and a concrete
  `rbxoc api show <endpoint-id>` command. 401/403 hints come from the endpoint
  that failed. Uncertain mutations, redirects, read-only queries, and
  feature-enrollment errors keep their existing guidance.
- `place update` confirms `serverSize` against the returned Place before it
  reports success. A missing or different value is `place_response_mismatch`
  and the PATCH is not sent again.
- `analytics metrics` turns a documented operation path into a pollable
  operation id. A path for another universe or kind is not rewritten into a
  command.
- Open Cloud errors surface `errorMessage`, nested `error.message`, and a JSON
  string body in the stable summary. The HTTP code and raw detail stay put.
- `badge create` sends `expectedCost`, `isActive`, and an optional `files`
  icon. Missing cost, missing active state, or an invalid icon fails locally
  and sends nothing.

## 0.6.1

Distribution-only patch. No CLI behavior, command surface, API contract, or
credential handling changed; the crate contents differ only in the version and
this changelog.

- Publish prebuilt archives, so a Roblox project can install rbxoc through
  Rokit alongside `rojo`, `stylua`, and `selene` instead of needing Rust 1.99
  and a native C toolchain. Previously every GitHub Release from `v0.3.4` to
  `v0.6.0` had an empty asset list, so
  `rbxoc = "eiei114/rbxoc@<version>"` failed with `No compatible artifact
  found`.
- Add a `macos-x86_64` archive. Rokit validates the operating system of a
  downloaded binary but not its architecture, so an Intel Mac previously
  installed the aarch64 build, reported success, and then could not execute.
- Package only the assets a release does not already contain. `gh release
  upload` rejects an entire batch when any single name exists, which made
  adding one archive to an existing release impossible.
- Take the packaging script from the dispatch commit rather than the release
  tag, so backfilling an older tag does not run that tag's stale copy.

Approved archive matrix: Windows x86_64 (`.zip`), Linux x86_64 (`.tar.gz`),
macOS aarch64 (`.tar.gz`), macOS x86_64 (`.tar.gz`).

## 0.6.0

- Redirects are never followed. A `307`/`308` can no longer replay a mutation,
  and `x-api-key` is never forwarded to a redirect target. A refused redirect
  returns `unexpected_redirect` with actionable hints.
- HTTP 408 on a mutation is treated as an uncertain outcome: the status is
  preserved, no automatic replay happens, and the hints ask for reconciliation
  rather than a blind retry. Safe reads keep their bounded retries.
- A read-only POST query is no longer classified as an unknown mutation.
  `analytics metrics` transient failures return `read_outcome_unknown` with
  truthful retry guidance while staying sent-at-most-once.
- New `place update` changes a place's `displayName`, `description` and
  `serverSize` with an `updateMask` built only from the supplied flags, plus a
  credential-free `--dry-run`. The root place is the documented write path
  because `Universe.displayName` and `Universe.description` are read-only.
- `place publish` rejects `.rbxl`/`.rbxlx` files above the documented 10 MiB
  `x-roblox-size-limit` locally, before any request is built. A file exactly at
  the limit stays eligible.
- `thumbnail delete` parses the documented per-`thumbnailId` boolean map. Only
  an explicit `true` counts as removed, so a mixed result reports
  `data.partial` and a result with no confirmed deletion fails as
  `thumbnail_delete_failed`. The provider receipt is preserved in `data` either
  way.
- `server list` exposes the provider's partial-result signals as `data.partial`
  and `data.missingSources` instead of summarizing an incomplete page as a
  complete success.
- The analytics alert family tracks the current contract path
  `/resource/{resourceType}/id/{resourceId}/alerts`. The OpenAPI auth fixture is
  refreshed (2026-10-23, sha256 `bcfdc206...`), and the catalog contract test
  fails when an implemented route disappears from the snapshot or when an
  analytics operation `kind` the CLI accepts has no documented route.

## 0.5.0

- DataStore value updates read the current entry and send `value`, `attributes`,
  `users`, and `etag` together. A missing etag or a forbidden read does not
  PATCH. The read also needs `universe-datastores.objects:read`.
- `gamepass update --price` changes only the price. Sale state changes only
  with `--is-for-sale`.
- Place publish sends `.rbxlx` as `application/xml` and `.rbxl` as
  `application/octet-stream`.
- Config repositories, draft documents, and analytics queries are checked
  against the closed schema before authentication. Config publish requires the
  reviewed draft hash.
- Permission subject `all` sends an empty subject id. Other subject types
  require a positive id.
- `publish-message` sends the exact string, unwrapping a JSON string once.
- Standalone asset create and update reject missing, unreadable, empty, and
  over-20-MiB files before authentication, including dry-run.
- Paginated lists forward the provider's continuation token. `server logs
  --limit` is `MaxPageSize`.
- Config restore and asset archive send no JSON body. Server restart still
  sends `{}`.
- Failures keep the leaf command that was run.

## 0.4.2

- Adopt the framed, transparent-background project icon and display it in the
  README; include the icon in the crate archive.
- Publish the MIT OR Apache-2.0 dual-license metadata and full license texts.
  Previously published releases retain their original license metadata.
- No command, authentication, networking or retry behavior changes.
