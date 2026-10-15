# rbxoc 0.3.4

Patch release addressing every open issue (#2–#8) in the asset publication and
registry workflow.

## Fixes and improvements

- **#2 — Decal versus Image IDs:** preserve original upload/registry IDs,
  distinguish caller-supplied Image IDs, provide the explicit offline
  `asset image-id` Studio fallback and `assets register --image-id`, and make
  image-reference generation opt-in with `codegen.image_ids = true`.
- **#3 — PNG preprocessing:** retain the actual `.png` upload format after SVG
  rasterization or alpha bleed, without changing source keys or cleanup.
- **#4 — Safe resume:** persist per-file create intent, operation IDs, and
  successful receipts with atomic writes. Poll known operations on retry,
  reject changed recovery inputs, and refuse uncertain automatic creates.
- **#5 — Scoped sync:** select exact normalized keys with repeatable `--path`;
  unknown keys fail, unselected entries never upload, and codegen keeps
  registered references without inventing IDs for unselected files.
- **#6 — Freshness:** track receipt-backed source/preprocessing fingerprints;
  report changed and legacy/manual unverified entries without silently
  publishing replacements or changing IDs.
- **#7 — Codegen collisions:** reject ambiguous flat/nested keys and leaf/table
  conflicts in Luau and TypeScript before replacing generated output.
- **#8 — Full local preflight:** validate the entire selected batch, prepared
  upload formats, creator/configuration, prospective registry, and output
  destination before the first cloud mutation. Prepared files stay on disk,
  not all in memory.

Generated output cannot overwrite source files, the manifest, receipts, or
credential/state files. Registry mutation is process-locked. Existing version-1
lockfiles and default Decal consumers remain compatible.

## Important boundaries

- `assets sync --dry-run` is a registry/known-freshness check, **not** full
  upload preflight, a remote-content check, or a proof of legacy ID freshness.
- Local preflight is not a cloud transaction. No rollback or exactly-once
  promise is made for lost server responses.
- Decal-to-Image automatic Open Cloud resolution is not implemented. The
  documented Studio fallback interprets a supplied Texture and explicitly
  reports no network validation. Live ImageLabel verification was not performed
  for this release; the reproducible offline fixtures are not a substitute.
- Existing IDs are not silently updated/replaced. Manual registration/import
  does not establish publication freshness.

## Install

```bash
cargo install rbxoc --version 0.3.4 --locked
```

See [asset sync safety](asset-sync-safety.md) and [Image IDs](image-ids.md) for
selection, recovery, explicit replacement, and Studio verification steps.
