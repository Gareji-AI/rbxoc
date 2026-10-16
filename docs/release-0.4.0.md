# rbxoc 0.4.0

Minor release implementing all eight open issues in the release snapshot:
#10, #11, #12, #13, #14, #15, #16, and #18.

## New: update registered Models without replacing their IDs (#18)

```bash
rbxoc assets sync --update --path assets/models/tree.fbx --dry-run
rbxoc assets sync --update --path assets/models/tree.fbx
```

- Explicit paths and changed, receipt-backed Model content are required.
  Missing, current, unverified, unsupported, wrong-creator, and duplicate-target
  selections fail before the first cloud mutation.
- Content-only PATCH follows the documented Assets API contract. Existing IDs
  and metadata stay unchanged. Successful matching operation receipts establish
  a fresh fingerprint and `current` registry state.
- Per-file update intent records the exact target asset ID. Known operations
  resume by polling; unknown outcomes block blind retries, even with `--retry`.
  Mismatched completion IDs never overwrite the original registry ID.
- Version-1 lockfiles and old create receipts remain compatible. After a partial
  batch, resume only unfinished paths with `--update --path`.

## Fixes and safety improvements

- **#10:** Unicode-safe masking counts characters; secrets of at most eight
  characters stay fully masked. Multibyte credentials no longer panic.
- **#14:** Credential resolution returns the actual selected source. Explicit
  key files, empty environment values, and OAuth precedence report truthfully.
- **#11:** Invalid CLI arguments return a JSON `usage_error` envelope on stdout
  with exit 64, separate from missing-auth exit 2. Help/version remain exit 0;
  the exit table now documents internal output failure code 70.
- **#15:** One shared helper encodes raw UTF-8 names in every named URL segment.
  URL delimiters stay data, dot-only/empty names fail locally, and alert/status
  query parameters are encoded separately.
- **#16:** Both live sweep entry points share a single reviewed case list.
  Default is read-only; targets have no committed production defaults. Mutations
  require exact `RBXOC_SWEEP_MUTATE=1`, target review is printed first, and the
  entire configuration is checked before any child command. Stale thumbnail and
  analytics syntax is corrected; unsupported `devproduct update` was removed.
- **#12:** CI runs the catalog coverage checker after the release build, plus
  process-level usage contracts and offline live-sweep safety tests. Historical
  endpoint counts now point to current discovery output.
- **#13:** README distinguishes live-verified reads, scope-limited implementations,
  and unverified mutations. The missing-auth example matches actual output and
  no longer recommends a home-directory credential fallback.

## Compatibility and verification boundaries

Usage errors intentionally change from clap's exit 2/text diagnostics to exit
64/JSON stdout. Consumers should branch on `error.code` and documented exits.
Normal sync still does not automatically update registered files; `--update`
is explicit and Model-only. Manual registration still does not prove freshness.

Local HTTP fixtures verify stable IDs, successful receipts, interruption,
uncertain outcomes, target/source/creator identity, unsupported content, and
partial-batch recovery. **No live Roblox mutations were performed for this
release.** FBX-derived Model moderation, latest-version delivery, and
`LoadAssetAsync` behavior remain live-unverified. Offline success is not an
exactly-once or cloud-transaction guarantee.

See [asset sync safety and recovery](https://github.com/eiei114/rbxoc/blob/v0.4.0/docs/asset-sync-safety.md)
and the [historical live verification report](https://github.com/eiei114/rbxoc/blob/v0.4.0/docs/live-api-verification.md).

## Install

```bash
cargo install rbxoc --version 0.4.0 --locked
```
