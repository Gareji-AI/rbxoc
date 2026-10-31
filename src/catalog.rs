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
//! Source: https://create.roblox.com/docs/cloud/openapi.json (auth/scopes fetched 2026-10-17)

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
    /// Documented operation permissions: OAuth security scopes plus
    /// x-roblox-scopes. Relevant to API-key permission hints as well.
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
        scopes: &[],
        mutating: false,
        note: "The smallest read, used as a connectivity probe.",
    },
    Endpoint {
        id: "universe.get",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}",
        summary: "Fetch a universe's metadata",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &[],
        mutating: false,
        note: "Also used by auth check when a universe is supplied.",
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
        note: ".rbxlx uses application/xml and .rbxl uses application/octet-stream. versionType=Saved only saves; Published also goes live.",
    },
    Endpoint {
        id: "place.get",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}/places/{place_id}",
        summary: "Fetch a place's metadata",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &[],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "place.update",
        method: "PATCH",
        path: "/cloud/v2/universes/{universe_id}/places/{place_id}",
        summary: "Change a place's name, description, or server size",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.place:write"],
        mutating: true,
        note: "updateMask carries only the supplied fields. The root place's displayName and description are what `universe get` reports, because Universe.displayName and Universe.description are read-only. A Team Create session returns 409.",
    },
    // ---- assets ----
    Endpoint {
        id: "asset.create",
        method: "POST",
        path: "/assets/v1/assets",
        summary: "Upload a new asset",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:read", "asset:write"],
        mutating: true,
        note: "Up to 20 MB per request. Models accept fbx / gltf / glb / rbxm / rbxmx. Image uploads return Decal IDs, not resolved Image IDs; see asset image-id.",
    },
    Endpoint {
        id: "asset.update",
        method: "PATCH",
        path: "/assets/v1/assets/{assetId}",
        summary: "Update an asset's content and metadata",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:read", "asset:write"],
        mutating: true,
        note: "Only Models can replace the content body. Use assets sync --update --path for receipt-backed changed Models under stable IDs. Content-only updates omit metadata updateMask. Icons and previews must be Image assets.",
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
        scopes: &["asset:read", "asset:write"],
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
        scopes: &["asset:read", "asset:write"],
        mutating: true,
        note: "The provider declares no request body.",
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
    Endpoint {
        id: "asset.registry",
        method: "LOCAL",
        path: "rbxoc.assets.toml",
        summary: "Generate Luau or TypeScript that references asset IDs by name",
        implemented: true,
        auth: &[],
        scopes: &[],
        mutating: true,
        note: "Local registry + sync. Exact repeatable sync --path selection, full local publication preflight, durable per-file receipts and recovery. Default sync creates missing files. Explicit --update --path updates only changed receipt-backed Models in place; its dry-run plans local updates without credentials. Normal dry-run checks registry/known freshness only. Register --image-id and codegen.image_ids are explicit offline Image reference options.",
    },
    Endpoint {
        id: "asset.image-id",
        method: "LOCAL",
        path: "Decal.Texture",
        summary: "Interpret a supplied Studio Decal.Texture as an Image ID",
        implemented: true,
        auth: &[],
        scopes: &[],
        mutating: false,
        note: "Offline fallback, not an Open Cloud lookup. Requires --decal-id and a --texture from Studio; without it returns unresolved instructions. Preserves original Decal IDs and never uploads or claims remote validation.",
    },
    // ---- thumbnails ----
    Endpoint {
        id: "thumbnail.upload",
        method: "POST",
        path: "/thumbnail-personalization-api/v1/universes/{universeId}/thumbnails/uploads",
        summary: "Replace a universe's homepage thumbnails",
        implemented: true,
        auth: &[ApiKey],
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
        auth: &[ApiKey],
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
        auth: &[ApiKey],
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
        auth: &[ApiKey],
        scopes: &["universe.thumbnail:write"],
        mutating: true,
        note: "Experimental. Repeat the homepageThumbnailIds query parameter for each ID; no JSON body.",
    },
    // ---- servers and logs ----
    Endpoint {
        id: "server.logs",
        method: "GET",
        path: "/server-management/v1/universes/{universeId}/places/{placeId}/versions/{versionNumber}/game-servers/{jobId}/logs",
        summary: "Fetch logs for a game server job",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
        mutating: false,
        note: "MaxPageSize is 1–100. --limit maps to that query; --full follows nextPageToken.",
    },
    Endpoint {
        id: "server.list",
        method: "GET",
        path: "/server-management/v1/universes/{universeId}/places/{placeId}/versions/{versionNumber}/game-servers",
        summary: "List game servers for a place version",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
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
    Endpoint {
        id: "server.restart-forecast",
        method: "GET",
        path: "/server-management/v1/universes/{universeId}/restarts:forecast",
        summary: "Forecast a universe server restart's impact without restarting",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
        mutating: false,
        note: "Also used by server restart --dry-run. Sends no restart mutation.",
    },
    // ---- analytics ----
    Endpoint {
        id: "analytics.metrics",
        method: "POST",
        path: "/analytics-query-api/v1/universes/{universeId}/metrics",
        summary: "Query time-series metrics",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.analytics:read"],
        mutating: false,
        note: "Not covered by rbxcloud. Supports API keys and OAuth2; enable universe.analytics:read.",
    },
    Endpoint {
        id: "analytics.operation",
        method: "GET",
        path: "/analytics-query-api/v1/universes/{universeId}/operations/{kind}/{operationId}",
        summary: "Fetch the result of an asynchronous analytics query",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.analytics:read"],
        mutating: false,
        note: "kind is metrics or dimension-values; CLI --kind dimensions maps to dimension-values.",
    },
    Endpoint {
        id: "analytics.dimensions",
        method: "POST",
        path: "/analytics-query-api/v1/universes/{universeId}/dimension-values",
        summary: "Query dimension values",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.analytics:read"],
        mutating: false,
        note: "Lists the values a metric's dimensions take, for breakdown and filter. Not covered by rbxcloud.",
    },
    // ---- place version history ----
    Endpoint {
        id: "place.history",
        method: "GET",
        path: "/place-version-history-api/v1/{placeId}/history",
        summary: "Fetch a place's version history",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe.place:read"],
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
        auth: &[ApiKey],
        scopes: &["universe-datastores.objects:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "datastore.set",
        method: "PATCH",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries/{entry_id}",
        summary: "Create or update a DataStore entry",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe-datastores.objects:update"],
        mutating: true,
        note: "Reads the entry first, which also needs universe-datastores.objects:read, then sends value, attributes, users, and etag. A missing etag fails locally. --create sends allowMissing=true.",
    },
    Endpoint {
        id: "datastore.delete",
        method: "DELETE",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries/{entry_id}",
        summary: "Delete a DataStore entry",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe-datastores.objects:delete"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "datastore.list",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries",
        summary: "List DataStore entries",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe-datastores.objects:list"],
        mutating: false,
        note: "",
    },
    // ---- ordered datastores (leaderboards) ----
    Endpoint {
        id: "ordered-datastore.list",
        method: "GET",
        path: "/ordered-data-stores/v1/universes/{universeId}/orderedDataStores/{orderedDataStore}/scopes/{scope}/entries",
        summary: "List ordered DataStore entries, sorted by value",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe.ordered-data-store.scope.entry:read"],
        mutating: false,
        note: "Backs leaderboards. The scope segment is required.",
    },
    Endpoint {
        id: "ordered-datastore.get",
        method: "GET",
        path: "/ordered-data-stores/v1/universes/{universeId}/orderedDataStores/{orderedDataStore}/scopes/{scope}/entries/{entry}",
        summary: "Read one ordered DataStore entry",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe.ordered-data-store.scope.entry:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "ordered-datastore.set",
        method: "PATCH",
        path: "/ordered-data-stores/v1/universes/{universeId}/orderedDataStores/{orderedDataStore}/scopes/{scope}/entries/{entry}",
        summary: "Create or replace an ordered DataStore entry",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe.ordered-data-store.scope.entry:write"],
        mutating: true,
        note: "Values are integers, not JSON blobs. --create sends allow_missing=true; otherwise updates only.",
    },
    Endpoint {
        id: "ordered-datastore.delete",
        method: "DELETE",
        path: "/ordered-data-stores/v1/universes/{universeId}/orderedDataStores/{orderedDataStore}/scopes/{scope}/entries/{entry}",
        summary: "Delete an ordered DataStore entry",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe.ordered-data-store.scope.entry:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "ordered-datastore.increment",
        method: "POST",
        path: "/ordered-data-stores/v1/universes/{universeId}/orderedDataStores/{orderedDataStore}/scopes/{scope}/entries/{entry}:increment",
        summary: "Add to an ordered DataStore entry atomically",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe.ordered-data-store.scope.entry:write"],
        mutating: true,
        note: "Read-modify-write races are avoided by the server.",
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
    Endpoint {
        id: "devproduct.create",
        method: "POST",
        path: "/developer-products/v2/universes/{universeId}/developer-products",
        summary: "Create a Developer Product",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["developer-product:write"],
        mutating: true,
        note: "Multipart. imageFile is optional. Beta; sent at most once.",
    },
    // ---- badges ----
    Endpoint {
        id: "badge.create",
        method: "POST",
        path: "/legacy-badges/v1/universes/{universeId}/badges",
        summary: "Create a badge",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["legacy-universe.badge:manage-and-spend-robux"],
        mutating: true,
        note: "Spends Robux. Multipart fields are name, description, paymentSourceType 1 (user) or 2 (group), numeric expectedCost, boolean isActive, and optional files icon. Missing cost, active state, or an invalid icon fails locally before any request. Sent at most once.",
    },
    Endpoint {
        id: "badge.update",
        method: "PATCH",
        path: "/legacy-badges/v1/badges/{badgeId}",
        summary: "Update a badge's configuration",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &[
            "legacy-universe.badge:manage-and-spend-robux",
            "legacy-universe.badge:write",
        ],
        mutating: true,
        note: "",
    },
    // ---- asset permissions ----
    Endpoint {
        id: "permissions.grant",
        method: "PATCH",
        path: "/asset-permissions-api/v1/assets/permissions",
        summary: "Grant a user, group, roleset, universe, or everyone access to assets",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["asset-permissions:write"],
        mutating: true,
        note: "Subject types: User, Group, GroupRoleset, Universe, All. All sends an empty subjectId. Actions: Use, Edit, Download.",
    },
    // ---- creator configs (remote config / feature flags) ----
    Endpoint {
        id: "config.get",
        method: "GET",
        path: "/creator-configs-public-api/v1/configs/universes/{universeId}/repositories/{repository}/full",
        summary: "Read the published config values",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
        mutating: false,
        note: "These are the values the running game reads.",
    },
    Endpoint {
        id: "config.draft",
        method: "GET",
        path: "/creator-configs-public-api/v1/configs/universes/{universeId}/repositories/{repository}/draft",
        summary: "Read the current draft",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "config.patch-draft",
        method: "PATCH",
        path: "/creator-configs-public-api/v1/configs/universes/{universeId}/repositories/{repository}/draft",
        summary: "Patch the draft without affecting live values",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:write"],
        mutating: true,
        note: "Not live until config publish runs.",
    },
    Endpoint {
        id: "config.overwrite",
        method: "PUT",
        path: "/creator-configs-public-api/v1/configs/universes/{universeId}/repositories/{repository}/draft:overwrite",
        summary: "Replace the entire draft",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "config.publish",
        method: "POST",
        path: "/creator-configs-public-api/v1/configs/universes/{universeId}/repositories/{repository}/publish",
        summary: "Publish the draft so the game reads it",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:write"],
        mutating: true,
        note: "Requires the reviewed draftHash. Optional message and deploymentStrategy Immediate or GradualRollout.",
    },
    Endpoint {
        id: "config.revisions",
        method: "GET",
        path: "/creator-configs-public-api/v1/configs/universes/{universeId}/repositories/{repository}/revisions",
        summary: "List config revision history",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "config.restore",
        method: "POST",
        path: "/creator-configs-public-api/v1/configs/universes/{universeId}/repositories/{repository}/revisions/{revisionId}/restore",
        summary: "Stage a revert to a previous revision",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:write"],
        mutating: true,
        note: "Stages only; publish to apply. The provider declares no request body.",
    },
    // ---- messaging ----
    Endpoint {
        id: "messaging.publish",
        method: "POST",
        path: "/messaging-service/v1/universes/{universeId}/topics/{topic}",
        summary: "Publish a cross-server message to a universe",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe-messaging-service:publish"],
        mutating: true,
        note: "message is the exact string, at most 1,024 characters. A JSON string is unwrapped once.",
    },
    // ---- analytics alerts ----
    Endpoint {
        id: "analytics.alert.list",
        method: "GET",
        path: "/analytics-alert-control-plane/v1/resource/{resourceType}/id/{resourceId}/alerts",
        summary: "List metric alert configurations for a universe",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.analytics.alert:read"],
        mutating: false,
        note: "resourceType is the case-sensitive provider value Universe.",
    },
    Endpoint {
        id: "analytics.alert.create",
        method: "POST",
        path: "/analytics-alert-control-plane/v1/resource/{resourceType}/id/{resourceId}/alerts",
        summary: "Create a metric alert configuration",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.analytics.alert:write"],
        mutating: true,
        note: "Takes a JSON document; the schema is provider-defined.",
    },
    Endpoint {
        id: "analytics.alert.update",
        method: "PATCH",
        path: "/analytics-alert-control-plane/v1/resource/{resourceType}/id/{resourceId}/alerts/{alertId}",
        summary: "Update a metric alert configuration",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.analytics.alert:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "analytics.alert.delete",
        method: "DELETE",
        path: "/analytics-alert-control-plane/v1/resource/{resourceType}/id/{resourceId}/alerts/{alertId}",
        summary: "Delete a metric alert configuration",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.analytics.alert:write"],
        mutating: true,
        note: "Irreversible.",
    },
    Endpoint {
        id: "analytics.alert.incidents",
        method: "GET",
        path: "/analytics-alert-control-plane/v1/resource/{resourceType}/id/{resourceId}/alerts/incidents",
        summary: "List alert incidents in a time window",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.analytics.alert:read"],
        mutating: false,
        note: "startTime and endTime are required.",
    },
    // ---- virtual events ----
    Endpoint {
        id: "event.list",
        method: "GET",
        path: "/virtual-events/v3/universes/{universeId}/game-events",
        summary: "List game events under a universe",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.event:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "event.get",
        method: "GET",
        path: "/virtual-events/v3/game-events/{eventId}",
        summary: "Get one game event",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.event:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "event.create",
        method: "POST",
        path: "/virtual-events/v3/universes/{universeId}/game-events",
        summary: "Create a game event",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.event:write"],
        mutating: true,
        note: "Takes a JSON document; the schema is provider-defined.",
    },
    Endpoint {
        id: "event.update",
        method: "PATCH",
        path: "/virtual-events/v3/game-events/{eventId}",
        summary: "Update a game event",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.event:write"],
        mutating: true,
        note: "Null means no change for a field.",
    },
    Endpoint {
        id: "event.delete",
        method: "DELETE",
        path: "/virtual-events/v3/game-events/{eventId}",
        summary: "Delete a game event",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.event:write"],
        mutating: true,
        note: "Permanent.",
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

/// Creator Dashboard permission group label for scope hints.
pub fn dashboard_permission_group(scopes: &[&str]) -> Option<&'static str> {
    let primary = scopes.first()?;
    if primary.starts_with("universe.analytics.alert") {
        return Some("universe-analytics-alert");
    }
    if primary.starts_with("universe-datastores") {
        return Some("universe-datastores");
    }
    if primary.starts_with("universe.analytics") {
        return Some("universe-analytics");
    }
    if primary.starts_with("universe.thumbnail") {
        return Some("universe-thumbnail");
    }
    if primary.starts_with("universe.event") {
        return Some("universe-event");
    }
    if primary.starts_with("game-pass") {
        return Some("game-pass");
    }
    if primary.starts_with("developer-product") {
        return Some("developer-product");
    }
    if primary.starts_with("asset-permissions") {
        return Some("asset-permissions");
    }
    if primary.starts_with("legacy-universe.badge") {
        return Some("legacy-universe-badge");
    }
    if primary.starts_with("universe-places") {
        return Some("universe-places");
    }
    if primary.starts_with("asset:") {
        return Some("asset");
    }
    if primary.starts_with("universe:") || *primary == "universe:read" {
        return Some("universe");
    }
    if primary.starts_with("creator-store-product") {
        return Some("creator-store-product");
    }
    None
}

/// Scopes and dashboard grouping for an endpoint ID.
pub fn permission_hints_for_endpoint_id(
    id: &str,
) -> Option<(&'static [&'static str], Option<&'static str>)> {
    let endpoint = find(id)?;
    if endpoint.scopes.is_empty() {
        return None;
    }
    Some((endpoint.scopes, dashboard_permission_group(endpoint.scopes)))
}

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
            // Local operations name a file; API entries name an HTTP path.
            let local = e.method == "LOCAL";
            assert!(
                local || e.path.starts_with('/'),
                "{} path must start with / for an HTTP endpoint",
                e.id
            );
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
