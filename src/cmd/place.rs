//! Place publishing and retrieval.
//!
//! `POST /universes/v1/{universeId}/places/{placeId}/versions` sends an RBXL /
//! RBXLX file as `data-binary`. `versionType` of `Saved` only saves;
//! `Published` also goes live.
//!
//! The response is the new place version number. A malformed file yields HTTP 400.

use serde_json::json;

use crate::ctx::{Ctx, failure};
use crate::output::Output;

const COMMAND: &str = "place publish";

/// Save or publish a place.
pub async fn publish(
    ctx: &Ctx,
    universe_id: u64,
    place_id: u64,
    file: &std::path::Path,
    published: bool,
) -> Output {
    let bytes = match tokio::fs::read(file).await {
        Ok(b) => b,
        Err(e) => {
            return failure(
                COMMAND,
                crate::output::ErrorBody {
                    code: "file_unreadable",
                    message: format!("cannot read the file: {}", file.display()),
                    status: None,
                    detail: Some(e.to_string()),

                    refs: None,
                },
            );
        }
    };

    let version_type = if published { "Published" } else { "Saved" };
    let path = format!(
        "/universes/v1/{universe_id}/places/{place_id}/versions?versionType={version_type}"
    );
    let req = ctx.client.post(ctx.url(&path)).body(bytes);

    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure(COMMAND, err);
            }
            let body = resp.text().await.unwrap_or_default();
            // The response is the version number itself, not JSON.
            let version: Option<u64> = body.trim().parse().ok();
            let action = if published { "published" } else { "saved" };
            let summary = match version {
                Some(v) => format!("Place {action} (version {v})"),
                None => format!("Place {action}"),
            };
            let mut hints =
                vec!["Run `rbxoc place history --place-id <id>` to inspect the history".into()];
            if !published {
                hints.push("Add --published to go live".into());
            }
            Output::success(
                COMMAND,
                summary,
                Some(json!({
                    "universeId": universe_id,
                    "placeId": place_id,
                    "version": version,
                    "versionType": version_type,
                    "action": action,
                })),
            )
            .with_hints(hints)
        }
        Err(err) => failure(COMMAND, err),
    }
}

/// Fetch a place's metadata.
pub async fn get(ctx: &Ctx, universe_id: u64, place_id: u64) -> Output {
    let path = format!("/cloud/v2/universes/{universe_id}/places/{place_id}");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let name = data
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("(unnamed)");
            Output::success("place get", format!("Fetched place: {name}"), Some(data))
        }
        Err(err) => failure("place get", err),
    }
}

/// Fetch a place's version history.
pub async fn history(ctx: &Ctx, place_id: u64, limit: u32) -> Output {
    let path = format!("/place-version-history-api/v1/{place_id}/history?limit={limit}");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("versions")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "place history",
                format!("Fetched {count} versions for place {place_id}"),
                Some(data),
            )
        }
        Err(err) => failure("place history", err),
    }
}
