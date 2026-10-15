# Asset sync: selection, freshness, and recovery

`assets sync` creates missing assets. It does not silently replace existing
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

Changed files are reported without re-uploading or changing their IDs. Use an
explicit `asset update` for supported Model content, or an explicit create and
register replacement. A manual registration clears freshness proof, so the
entry stays unverified until there is a matching successful sync receipt.
This is publication freshness, not a general build cache.

## Crash-safe per-file receipts

The lockfile retains its version-1 `assets` map and adds optional `pending`
and `publications` tables. Receipts contain only source fingerprints, creator,
asset type, operation ID, and returned asset ID, never API keys or tokens.
Commit successful receipts with the lockfile. Do not delete pending records to
make a failed command appear clean.

Writes use a flushed temporary file in the same directory and atomic
replacement (plus parent-directory sync on Unix). A process lock in
`.rbxoc/assets-sync.lock` prevents concurrent registry mutations. A crash may
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

Recovery rejects changed bytes, preprocessing settings, asset type, or creator.
Restore the original source/configuration before resuming. This prevents a
stale operation from being attached to different content.

## Uncertain outcomes need explicit reconciliation

If a create response is lost, malformed, or lacks an operation ID, sync leaves
an intent with unknown outcome and returns `publication_uncertain`. It refuses
another create on retry. A 5xx response may also have an uncertain outcome.
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
