# Changelog

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
