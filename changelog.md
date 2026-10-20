# Changelog

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
