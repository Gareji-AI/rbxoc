# rbxoc — finish everything except live API verification

Extend `rbxoc` to cover the remaining practical Open Cloud domains, make the
toolchain consistent, and record the research. Live API calls stay out of
scope per the user's instruction.

Working directory: `C:/Users/Keisu/Projects/OSS/rbxoc`

## Goals

- Cover the remaining high-value Open Cloud domains with working commands
- Keep every command on the same agent-facing contract (envelope, hints, exit codes)
- Keep all text English
- Leave the toolchain consistent (mise, no rokit leftovers)
- Record the Open Cloud survey so the findings survive

## Checklist

### A. Ordered DataStore (highest value; datastore already exists)
- [x] `ordered-datastore` command: list, get, set, delete, increment
- [x] Catalog entries with auth/scopes/notes
- [x] Tests for path building

### B. Asset permissions
- [x] `permissions grant` (PATCH /asset-permissions-api/v1/assets/permissions)
- [x] Catalog entry

### C. Badges
- [x] `badge create`, `badge update`
- [x] Catalog entries (mark the Robux-spending scope)

### D. Creator configs (remote config / feature flags)
- [x] `config get`, `config draft`, `config overwrite`, `config publish`, `config revisions`
- [x] Catalog entries

### E. Analytics alerts
- [x] `analytics alerts list|create|update|delete`
- [x] Catalog entries

### F. Virtual events
- [x] `event list`, `event get`, `event create`, `event delete`
- [x] Catalog entries

### G. Toolchain consistency
- [x] Confirm `~/.rokit` is not referenced by rbxoc
- [x] Document the mise-only path
- [x] Verify `mise run check` is the single entry point

### H. Documentation
- [x] README: every new command with an example
- [x] README: coverage table vs rbxcloud
- [x] `docs/open-cloud-coverage.md`: the 648-path survey result
- [x] Note that unimplemented entries stay visible in the catalog

## Verification

Record commands, working directory, environment, and outputs here as work lands.

## Final Verification

- Exact monitor-rerunnable command: `cd C:/Users/Keisu/Projects/OSS/rbxoc && mise run check && ./target/release/rbxoc api list --implemented && node scripts/check-coverage.mjs`
- Working directory: `C:/Users/Keisu/Projects/OSS/rbxoc`
- Required preserved artifacts: `target/release/rbxoc.exe`, `Cargo.lock`
- Result: pending

## Notes

Update with progress, decisions, and blockers.

- 2026-10-13: loop started. Baseline is 29 implemented endpoints at commit 928ed49.
- Iteration 1: Ordered DataStore done (list/get/set/delete/increment).
  5 catalog entries added. 3 new unit tests. `mise run check` exit 0, 50/50 tests.
  Note: ordered DataStores require a scope segment, unlike DataStore v2.
- Scope excludes live API calls by explicit instruction.
