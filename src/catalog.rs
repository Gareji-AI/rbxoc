//! API catalog.
//!
//! The self-describing layer that lets agents discover what is possible
//! deterministically. `rbxoc api list` / `rbxoc api show <id>` read this.
//!
//! Every entry maps 1:1 to a real Open Cloud endpoint. The catalog is the
//! source of truth for the implementation, so add to it first. Entries with
//! `implemented: false` mean "the API exists but this CLI does not cover it
//! yet". That keeps agents from burning attempts on unsupported features.
//!
//! Source: https://create.roblox.com/docs/cloud/openapi.json (fetched 2026-10-12)

use serde::Serialize;

/// Authentication family. Open Cloud lists three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AuthKind {
    /// `x-api-key`. The first choice for automation.
    #[serde(rename = "api-key")]
    ApiKey,
    /// `Authorization: Bearer`. Areas that need scopes.
    #[serde(rename = "oauth")]
    OAuth,
    /// Browser session cookie. Out of scope for this CLI.
    #[serde(rename = "legacy-cookie")]
    LegacyCookie,
}

/// One endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct Endpoint {
    /// Stable `resource.action` ID. Matches the CLI subcommand name.
    pub id: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub summary: &'static str,
    /// Whether this CLI implements it. Reasons for `false` live in `note`.
    pub implemented: bool,
    pub auth: &'static [AuthKind],
    /// OAuth2 scopes. Unused when operating with an API key.
    pub scopes: &'static [&'static str],
    /// Whether the call mutates data. Lets agents ask before acting.
    pub mutating: bool,
    pub note: &'static str,
}

use AuthKind::{ApiKey, LegacyCookie, OAuth};

/// The catalog itself. Updated alongside the implementation.
pub const ENDPOINTS: &[Endpoint] = &[
    // ---- auth ----
    Endpoint {
        id: "auth.check",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}",
        summary: "Verify that the credentials can read a universe",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
        mutating: false,
        note: "The smallest read, used as a connectivity probe.",
    },
    // ---- place publishing ----
    Endpoint {
        id: "place.publish",
        method: "POST",
        path: "/universes/v1/{universeId}/places/{placeId}/versions",
        summary: "Save or publish a place from an RBXL / RBXLX file",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe-places:write"],
        mutating: true,
        note: "versionType=Saved only saves; Published also goes live.",
    },
    Endpoint {
        id: "place.get",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}/places/{place_id}",
        summary: "Fetch a place's metadata",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
        mutating: false,
        note: "",
    },
    // ---- assets ----
    Endpoint {
        id: "asset.create",
        method: "POST",
        path: "/assets/v1/assets",
        summary: "Upload a new asset",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:write"],
        mutating: true,
        note: "Up to 20 MB per request. Models accept fbx / gltf / glb / rbxm / rbxmx.",
    },
    Endpoint {
        id: "asset.update",
        method: "PATCH",
        path: "/assets/v1/assets/{assetId}",
        summary: "Update an asset's content and metadata",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:write"],
        mutating: true,
        note: "Only Models can replace the content body. Icons and previews must be Image assets.",
    },
    Endpoint {
        id: "asset.get",
        method: "GET",
        path: "/assets/v1/assets/{assetId}",
        summary: "Fetch asset metadata",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "asset.versions",
        method: "GET",
        path: "/assets/v1/assets/{assetId}/versions",
        summary: "List an asset's versions",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "asset.rollback",
        method: "POST",
        path: "/assets/v1/assets/{assetId}/versions:rollback",
        summary: "Roll an asset back to a given version",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "asset.archive",
        method: "POST",
        path: "/assets/v1/assets/{assetId}:archive",
        summary: "Archive an asset",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "asset.operation",
        method: "GET",
        path: "/assets/v1/operations/{operationId}",
        summary: "Check the status of an upload or update",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:read"],
        mutating: false,
        note: "create / update are asynchronous. Poll the operationId.",
    },
    // ---- thumbnails ----
    Endpoint {
        id: "thumbnail.upload",
        method: "POST",
        path: "/thumbnail-personalization-api/v1/universes/{universeId}/thumbnails/uploads",
        summary: "Replace a universe's homepage thumbnails",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.thumbnail:write"],
        mutating: true,
        note: "Experimental. Expect the schema to change without notice.",
    },
    Endpoint {
        id: "thumbnail.status",
        method: "GET",
        path: "/thumbnail-personalization-api/v1/universes/{universeId}/thumbnails/uploads/status",
        summary: "Check thumbnail upload processing status",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.thumbnail:read"],
        mutating: false,
        note: "Experimental.",
    },
    Endpoint {
        id: "thumbnail.list",
        method: "GET",
        path: "/thumbnail-personalization-api/v1/universes/{universeId}/thumbnails",
        summary: "List homepage thumbnails",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.thumbnail:read"],
        mutating: false,
        note: "Experimental.",
    },
    Endpoint {
        id: "thumbnail.delete",
        method: "DELETE",
        path: "/thumbnail-personalization-api/v1/universes/{universeId}/thumbnails",
        summary: "Delete homepage thumbnails",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.thumbnail:write"],
        mutating: true,
        note: "Experimental.",
    },
    // ---- servers and logs ----
    Endpoint {
        id: "server.logs",
        method: "GET",
        path: "/server-management/v1/universes/{universeId}/places/{placeId}/versions/{versionNumber}/game-servers/{jobId}/logs",
        summary: "Fetch logs for a game server job",
        implemented: true,
        auth: &[ApiKey],
        scopes: &[],
        mutating: false,
        note: "Not covered by rbxcloud. A differentiator for this CLI.",
    },
    Endpoint {
        id: "server.list",
        method: "GET",
        path: "/server-management/v1/universes/{universeId}/places/{placeId}/versions/{versionNumber}/game-servers",
        summary: "List game servers for a place version",
        implemented: true,
        auth: &[ApiKey],
        scopes: &[],
        mutating: false,
        note: "Take a jobId from here and pass it to server.logs.",
    },
    Endpoint {
        id: "server.restart",
        method: "POST",
        path: "/cloud/v2/universes/{universe_id}:restartServers",
        summary: "Restart a universe's game servers",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:write"],
        mutating: true,
        note: "Use forecast first to check the impact.",
    },
    // ---- analytics ----
    Endpoint {
        id: "analytics.metrics",
        method: "POST",
        path: "/analytics-query-api/v1/universes/{universeId}/metrics",
        summary: "Query time-series metrics",
        implemented: true,
        auth: &[OAuth],
        scopes: &["universe.analytics:read"],
        mutating: false,
        note: "Not covered by rbxcloud. Requires OAuth2, not an API key.",
    },
    Endpoint {
        id: "analytics.dimensions",
        method: "POST",
        path: "/analytics-query-api/v1/universes/{universeId}/dimension-values",
        summary: "Query dimension values",
        implemented: false,
        auth: &[OAuth],
        scopes: &["universe.analytics:read"],
        mutating: false,
        note: "Not implemented. Shaped like metrics; add later.",
    },
    // ---- place version history ----
    Endpoint {
        id: "place.history",
        method: "GET",
        path: "/place-version-history-api/v1/{placeId}/history",
        summary: "Fetch a place's version history",
        implemented: true,
        auth: &[ApiKey],
        scopes: &[],
        mutating: false,
        note: "Not covered by rbxcloud.",
    },
    Endpoint {
        id: "place.history.notes",
        method: "POST",
        path: "/place-version-history-api/v1/{placeId}/version/{version}/notes",
        summary: "Attach a note to a place version",
        implemented: false,
        auth: &[ApiKey],
        scopes: &[],
        mutating: true,
        note: "Not implemented.",
    },
    // ---- datastores ----
    Endpoint {
        id: "datastore.get",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries/{entry_id}",
        summary: "Read a DataStore entry",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe-datastores.control:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "datastore.set",
        method: "PATCH",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries/{entry_id}",
        summary: "Create or update a DataStore entry",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe-datastores.control:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "datastore.delete",
        method: "DELETE",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries/{entry_id}",
        summary: "Delete a DataStore entry",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe-datastores.control:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "datastore.list",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries",
        summary: "List DataStore entries",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe-datastores.control:read"],
        mutating: false,
        note: "",
    },
    // ---- monetization ----
    Endpoint {
        id: "gamepass.list",
        method: "GET",
        path: "/game-passes/v1/universes/{universeId}/game-passes/creator",
        summary: "List Game Passes",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["game-pass:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "gamepass.create",
        method: "POST",
        path: "/game-passes/v1/universes/{universeId}/game-passes",
        summary: "Create a Game Pass",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["game-pass:write"],
        mutating: true,
        note: "Multipart. imageFile can ride in the same request. Beta.",
    },
    Endpoint {
        id: "gamepass.update",
        method: "PATCH",
        path: "/game-passes/v1/universes/{universeId}/game-passes/{gamePassId}",
        summary: "Update a Game Pass",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["game-pass:write"],
        mutating: true,
        note: "Beta.",
    },
    Endpoint {
        id: "devproduct.list",
        method: "GET",
        path: "/developer-products/v2/universes/{universeId}/developer-products/creator",
        summary: "List Developer Products",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["developer-product:read"],
        mutating: false,
        note: "Beta.",
    },
    // ---- messaging ----
    Endpoint {
        id: "messaging.publish",
        method: "POST",
        path: "/messaging-service/v1/universes/{universeId}/topics/{topic}",
        summary: "Publish a cross-server message to a universe",
        implemented: true,
        auth: &[ApiKey],
        scopes: &[],
        mutating: true,
        note: "",
    },
    // ---- exists but unimplemented ----
    Endpoint {
        id: "media.get",
        method: "GET",
        path: "/v2/games/{universeId}/media",
        summary: "Fetch experience media data",
        implemented: false,
        auth: &[LegacyCookie],
        scopes: &[],
        mutating: false,
        note: "Not implemented. Cookie-only, so it does not suit automation.",
    },
    Endpoint {
        id: "creatorstore.get",
        method: "GET",
        path: "/toolbox-service/v2/assets/{id}",
        summary: "Fetch a Creator Store asset",
        implemented: false,
        auth: &[ApiKey, OAuth],
        scopes: &["creator-store-product:read"],
        mutating: false,
        note: "Not implemented.",
    },
];

/// Look up by ID.
pub fn find(id: &str) -> Option<&'static Endpoint> {
    ENDPOINTS.iter().find(|e| e.id == id)
}

/// Substring search. Helps agents explore.
pub fn search(query: &str) -> Vec<&'static Endpoint> {
    let q = query.to_lowercase();
    ENDPOINTS
        .iter()
        .filter(|e| {
            e.id.to_lowercase().contains(&q)
                || e.summary.to_lowercase().contains(&q)
                || e.path.to_lowercase().contains(&q)
        })
        .collect()
}

/// Implemented entries only. Used by `api list --implemented` and tests.
pub fn implemented() -> Vec<&'static Endpoint> {
    ENDPOINTS.iter().filter(|e| e.implemented).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = ENDPOINTS.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate endpoint IDs");
    }

    #[test]
    fn implemented_entries_have_no_placeholder_path() {
        for e in implemented() {
            assert!(!e.path.is_empty(), "{} has an empty path", e.id);
            assert!(e.path.starts_with('/'), "{} path must start with /", e.id);
        }
    }

    #[test]
    fn unimplemented_entries_explain_themselves() {
        for e in ENDPOINTS.iter().filter(|e| !e.implemented) {
            assert!(!e.note.is_empty(), "unimplemented {} has no reason", e.id);
        }
    }

    #[test]
    fn legacy_cookie_endpoints_are_not_implemented() {
        for e in ENDPOINTS {
            if e.auth == [LegacyCookie] {
                assert!(!e.implemented, "{} is cookie-only and out of scope", e.id);
            }
        }
    }

    #[test]
    fn find_and_search_work() {
        assert!(find("place.publish").is_some());
        assert!(find("nope.nope").is_none());
        let hits = search("thumbnail");
        assert!(hits.len() >= 4, "too few thumbnail results");
    }

    #[test]
    fn auth_kinds_serialize_to_stable_names() {
        let json = serde_json::to_string(&[ApiKey, OAuth, LegacyCookie]).unwrap();
        assert_eq!(json, r#"["api-key","oauth","legacy-cookie"]"#);
    }
}
