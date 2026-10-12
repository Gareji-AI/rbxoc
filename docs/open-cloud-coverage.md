# Open Cloud coverage

What `rbxoc` covers, what it deliberately does not, and how that was decided.

Survey date: 2026-10-12. Source: `create.roblox.com/docs/cloud/openapi.json`
(3,105,191 bytes, 648 paths), read directly rather than through the docs pages.

## How to re-check

```bash
node scripts/check-coverage.mjs
```

The checker parses `src/catalog.rs` and the release binary, then asserts:

- catalog IDs are unique
- every implemented entry has a path and a summary
- HTTP paths start with `/` (local operations may name a file)
- every unimplemented entry states why
- no cookie-only endpoint is implemented
- the binary reports the same entry count as the source

## Current state

58 catalog entries, 54 implemented.

| Domain | Implemented | Notes |
| --- | --- | --- |
| `asset` | 8 | create/update/get/versions/rollback/archive/operation + the local ID registry |
| `config` | 7 | remote config and feature flags, draft/publish |
| `analytics` | 6 | metrics query plus alert configurations |
| `event` | 5 | virtual events v3 |
| `ordered-datastore` | 5 | leaderboards |
| `datastore` | 4 | standard DataStores |
| `thumbnail` | 4 | experimental API |
| `place` | 3 | publish, get, version history |
| `server` | 3 | list, logs, restart |
| `gamepass` | 3 | list/create/update |
| `badge` | 2 | create/update |
| `auth` | 1 | credential probe |
| `devproduct` | 1 | list |
| `messaging` | 1 | cross-server publish |
| `permissions` | 1 | asset permission grants |

## Where rbxoc adds coverage over rbxcloud

`rbxcloud` (139 stars) last shipped 2025-03-23. Everything below landed after
that, so it is not covered there.

| Area | rbxcloud | rbxoc |
| --- | --- | --- |
| Thumbnail replacement | no | yes |
| Game server logs | no | yes |
| Analytics queries | no | yes |
| Analytics alerts | no | yes |
| Place version history | no | yes |
| Remote config | no | yes |
| Ordered DataStores | no | yes |
| Virtual events | no | yes |
| Asset permissions | no | yes |
| Badges | no | yes |
| Game Pass / Developer Product | no | yes |
| Asset ID codegen | no | yes |
| Place publishing | yes | yes |
| Asset upload | yes | yes |
| DataStore v2 | yes | yes |

## Deliberately out of scope

| Area | Why |
| --- | --- |
| Everything under `roblox-legacy-cookie` auth | Browser-session cookies do not suit automation, and the credential cannot be minted per task. |
| `/v2/games/{universeId}/media` | Cookie-only, so the same reason. |
| Mesh upload (`model/x-file-mesh-data`) | Publisher's API only accepts content downloaded from the asset delivery API. Studio's Importer is the real route. |
| 3D importer settings | Open Cloud does not expose them. This is a provider limit, not a CLI gap. |

Unimplemented entries stay visible in the catalog with `implemented: false`
and a reason, so an agent does not spend attempts on a command that does not
exist.

## Considered and deferred

| Area | Paths | Why deferred |
| --- | --- | --- |
| Ads management | 8 | Only relevant to teams buying ads. |
| Matchmaking | 15 | Narrows to games using custom matchmaking. |
| Creator Store / toolbox | 4 | Browse-shaped, and the useful parts need a cookie. |
| Localization tables | 42 | Large legacy family; no current demand. |
| `legacy-develop` / `legacy-publish` | 11 | Superseded by the v2 families already covered. |

## A note on GitHub search

Searching GitHub for a Roblox Open Cloud CLI surfaces several repositories
titled "Best ... 2026" with around 70 stars. Five of them share an identical
creation date (2026-09-22), an identical last-push date, zero forks, and the
same star count, with templated descriptions. They are SEO artifacts rather
than maintained projects.

The only substantial prior work is `Sleitnick/rbxcloud`. Judge an option by
commit history and forks, not stars alone.
