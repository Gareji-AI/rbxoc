# Asset sync: selection, freshness, and recovery

`assets sync` creates missing assets by default. Explicit `--update --path`
updates changed registered Models in place. Neither mode silently replaces existing
IDs, delete successful uploads as rollback, or promise a cloud transaction.

## Select exactly what can be published

```bash
rbxoc assets status --dir ./art
rbxoc assets sync --dir ./art --path ui/icon.svg --path animations/walk.rbxmx
rbxoc assets sync --dir ./art --path ui/icon.svg --dry-run
```

`--path` is repeatable and accepts exact, case-sensitive manifest/lock keys,
not filesystem paths or globs. Backslashes normalize to forward slashes. Use
the keys from `assets status`; for example, an input matching
`source/assets/ui/icon.svg` with `path_key_from = "assets/"` has key
`ui/icon.svg`. Every supplied key must exist. Unknown keys abort before any
upload, never fall back to full sync. Repeated keys are deduplicated.

Without `--path`, sync selects all manifest entries, as before. Already
registered selected entries keep their IDs. Unselected entries never cause a
cloud write. Codegen still includes **all registered entries**, including
orphaned entries, and omits unselected missing entries. The creator and
unrelated registry records are preserved. Two different source files cannot
share a normalized manifest key.

## Update changed Models without changing IDs

```bash
rbxoc assets sync --dir ./art --update --path models/tree.fbx --dry-run
rbxoc assets sync --dir ./art --update --path models/tree.fbx
rbxoc assets status --dir ./art
```

`--update` requires one or more explicit `--path` values. Every selected entry
must have a positive registered ID and a successful **Model** publication receipt
for that same ID and creator. Its source/preprocessing fingerprint must have
changed. Missing, unverified, and current entries are rejected locally, not
created or silently skipped. A creator change is not permission to update an
old creator's asset: restore the receipt's creator first. Two selected sources
may not target the same ID. New bytes must still infer as Model; Decal, Image,
Audio, Video and Animation updates are refused before the first cloud mutation.

The whole selection is validated/preprocessed before the first PATCH. Only
content is updated, not the display name or description. A completed operation
must return the **same asset ID**; a different/missing ID preserves the old ID
and pending receipt and fails. The new fingerprint is committed only after a
successful matching receipt; `assets status` then reports `current`. Codegen
keeps all registered IDs. No unselected file is updated or uploaded.

`--update --dry-run` needs no credentials, lists `updates` with path, asset ID,
type and `update`/`resume-update` action, and validates local eligibility,
preprocessing, type and size. It does not write the lockfile or generated output,
check destination writability, or prove cloud permissions/moderation/delivery.
This differs from the default registry-only dry-run below.

Interrupted updates use the same intent → operation → completion receipts as
creates, with a `target_asset_id` that must keep matching the registered ID.
Keep original bytes/settings/creator/target while recovering. A known operation
is polled, never blindly PATCHed again. An unknown operation outcome blocks
automatic retry, even with `--retry`. After a partial batch, select **only the
unfinished paths** when resuming; successfully updated paths are now `current`
and intentionally fail a new `--update` selection.

```bash
# If an update response was lost, attach only its independently known operation.
rbxoc assets recover --dir ./art --path models/tree.fbx --operation-id OPERATION_ID
rbxoc assets sync --dir ./art --update --path models/tree.fbx
```

Plain sync never resumes a pending update as a create. Standalone `asset update`
does not maintain registry receipts; use this scoped workflow when freshness
must remain known. Live in-place FBX-derived Model updates, moderation, and
`LoadAssetAsync` latest-version delivery remain **unverified**; local HTTP
fixtures prove CLI safety/recovery, not Roblox platform behavior.

The request follows Roblox's [Assets API content-only update sample](https://github.com/Roblox/creator-docs/blob/main/content/en-us/reference/cloud/assets/v1.json):
PATCH the existing asset path with multipart `request` containing `assetId` and
`fileContent`, without a metadata `updateMask`. Asset type is inferred locally
to enforce Model-only eligibility, not submitted as a metadata update.

## Full local preflight before publication

The upload path resolves the whole selection, validates creator/codegen
settings, reads/fingerprints each source, runs the same SVG/alpha-bleed
preprocessing and asset-type/model inference as upload, checks prepared size
(nonempty, at most 20 MiB), and checks prospective generated keys before the
first create request. Prospective collision checks need no cloud IDs and cover
both Luau/TypeScript and flat/nested output, including leaf/table conflicts.
Existing generated files remain intact on key collision errors.

Converted PNG temporary files carry `.png`, not the original SVG/BMP/TGA
extension. They stay on disk until their upload finishes and clean up on early
returns too. The batch does not retain every file's bytes in RAM. Sources are
checked again before their create request; concurrent input changes can still
interrupt the batch. Source keys always refer to the original input.

Local errors identify the source in the usual JSON error envelope. Network
failures, permissions that require remote checks, moderation, disk failures
after preflight, and concurrently modified inputs can still stop publication.
There is no all-or-nothing guarantee and no destructive rollback.

## Registry check versus proof of freshness

`assets sync --dry-run` requires no credentials and performs no cloud calls.
It is a **registry and known-freshness check**, not full preprocessing/upload
preflight or remote verification. It uses the same selection as real sync:
unselected missing or stale paths do not fail a scoped check.
It also performs a best-effort read-only advisory check on XML Animations at
least 2 MiB. That threshold is not a Roblox limit or a proven HTTP 500 cause;
no binary conversion or source/fingerprint changes are made. See README.

`assets status` and dry-run report:

- `missing`: no registered ID.
- `current`: a successful sync receipt matches the registered ID, source bytes,
  effective bleed setting, versioned preprocessing contract, and creator.
- `changed`: known publication content/settings/creator no longer match.
- `unverified`: a registered ID has no matching publication fingerprint.
  Existing version-1 lockfiles, `assets register`, and Asphalt imports remain
  readable and usable but do not certify remote freshness.
- `pending`: publication has not yet been reconciled with a completed receipt.

Dry-run fails for selected missing, changed, or pending entries. It may pass
with unverified entries, but explicitly reports them as unverified, never as
current. Fingerprints are written only after successful operation receipts;
sync never accepts edited local bytes as proof that an old ID was republished.

Default sync reports changed files without re-uploading or changing their IDs.
Use `assets sync --update --path` for receipt-backed changed Models, or an
explicit create and register replacement for other types. A standalone
`asset update` does not refresh the registry receipt. Manual registration clears
freshness proof, so the
entry stays unverified until there is a matching successful sync receipt.
This is publication freshness, not a general build cache.

## Crash-safe per-file receipts

The lockfile retains its version-1 `assets` map and adds optional `pending`
and `publications` tables. Receipts contain only source fingerprints, creator,
asset type, operation ID, returned asset ID, and (for updates) target asset ID,
never API keys or tokens. Older pending receipts without `target_asset_id`
remain create receipts, preserving version-1 lockfile compatibility.
Commit successful receipts with the lockfile. Do not delete pending records to
make a failed command appear clean.

Writes use a flushed temporary file in the same directory and atomic
replacement (plus parent-directory sync on Unix). A process lock in
`.rbxoc-assets-sync.lock` prevents concurrent registry mutations. Keep this
empty, persistent local guard out of version control (add it to `.gitignore`).
It deliberately does not create `.rbxoc`, which would change project-local
credential discovery boundaries. A crash may
leave an unused temporary file, but cannot expose a partially written lockfile.

For every missing file, sync:

1. Persists create intent before sending the request.
2. Persists the returned operation ID immediately, before polling.
3. Persists the completed asset ID and its fingerprint before starting the
   next file, then removes that pending entry.

If A succeeds and B fails, A stays registered. Retrying sync polls B's existing
operation and never creates another asset for it. Poll failures/timeouts leave
the operation receipt intact. Failed operations also remain recorded; retrying
polls them rather than silently issuing a replacement create.

On any post-preflight sync failure, codegen is refreshed from the **durable
on-disk lockfile** before returning the original failure. Completed A and other
registered IDs are usable even if B is uncertain; pending creates are not emitted.
Pending updates retain their already registered target IDs, not a new freshness
claim. `data.partialSync` lists the output path and registered map after a
successful refresh. Codegen failure adds a hint without hiding the publication
error or dropping receipts. Repair local output/receipt storage, then run
`rbxoc assets codegen` to regenerate without publishing. A failed completion
receipt save must never cause an unsaved in-memory ID to appear in generated code.
Preflight failures still leave existing generated output untouched.

Recovery rejects changed bytes, preprocessing settings, asset type, or creator.
Restore the original source/configuration before resuming. This prevents a
stale operation from being attached to different content.

## Uncertain outcomes need explicit reconciliation

If a create or update response is lost, malformed, or lacks an operation ID, sync leaves
an intent with unknown outcome and returns `publication_uncertain`. It refuses
another create or update on retry. A 5xx response may also have an uncertain outcome.
An explicit non-timeout 4xx rejection clears its unaccepted intent safely.

Inspect Creator Dashboard or your saved response, then choose the applicable
reconciliation. Do not guess an operation ID or asset ID:

```bash
# Inspect a known operation; read-only cloud request.
rbxoc asset operation --operation-id OPERATION_ID

# Attach a known operation to the existing uncertain intent; offline only.
rbxoc assets recover --dir ./art --path ui/icon.svg --operation-id OPERATION_ID
rbxoc assets sync --dir ./art --path ui/icon.svg

# If only the successful asset ID is known, record it without uploading.
# This is an explicit manual reconciliation, not freshness proof.
rbxoc assets register --dir ./art --path ui/icon.svg --id ASSET_ID
```

`assets recover` validates the original fingerprint/creator and refuses to
overwrite an already recorded, different operation. Only attach an operation
known to belong to that exact source and creator; the offline command cannot
independently prove the remote association.

Receipt-write errors return `receipt_write_failed` with known operation/asset
IDs in recovery hints. Preserve those values, restore writable local storage,
then recover/register explicitly. Do not start another upload to compensate.

Only after independently verifying a failed request created **no asset** may
you explicitly discard its pending record:

```bash
rbxoc assets unregister --dir ./art --path ui/icon.svg --discard-pending
```

Without `--discard-pending`, unregister refuses to forget pending uploads.
Discarding an unknown outcome can create duplicates on the next sync. Exactly
once publication is impossible to promise when the server accepted a request
but its response cannot be recovered.
