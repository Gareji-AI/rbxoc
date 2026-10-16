# rbxoc 0.4.1

Patch release fixing all nine API-contract and safety issues #20–#28.

- **#20:** Standard DataStore writes preserve every JSON value type without
  double encoding strings.
- **#21:** Automatic retries are limited to safe HTTP methods. Mutations are
  sent once; transport/429/5xx outcomes return `mutation_outcome_unknown` with
  reconciliation guidance. Existing publication receipt recovery is retained.
- **#22:** `--create` uses the documented query parameter: `allowMissing=true`
  for standard DataStores, `allow_missing=true` for ordered DataStores.
- **#23:** `server restart --dry-run` uses the GET restart forecast endpoint.
  It never sends a restart mutation or an unsupported `dryRun` field.
- **#24:** Badge creation uses multipart data and `paymentSourceType` enum
  values `1` (user) / `2` (group). Invalid payment sources fail locally.
  Creating a badge still spends Robux.
- **#25:** The catalog now includes Developer Product creation, restart
  forecasting, analytics operation results, and universe metadata reads.
  Coverage checks validate actual command mappings and mutation discovery,
  not only endpoint counts.
- **#26:** Thumbnail deletion uses repeated `homepageThumbnailIds` query
  parameters, no JSON body, and rejects empty IDs locally.
- **#27:** All failures include non-empty actionable hints, including local
  validation, registry, file, and response-parsing errors, in JSON and text.
- **#28:** All analytics alert operations share the case-sensitive `Universe`
  resource path.

## Verification

Offline HTTP fixtures assert methods, paths, query encoding, JSON/multipart
bodies, zero restart mutations for dry-run, and bounded/suppressed retries.
No live Open Cloud calls, paid badge creation, or production restarts are
needed for these regressions. Live provider behavior is not claimed by the
offline tests.

## Install after publication

```bash
cargo install rbxoc --version 0.4.1 --locked
```
