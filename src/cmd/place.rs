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
    let media_type = match place_media_type(file) {
        Ok(media_type) => media_type,
        Err(err) => return failure(COMMAND, err),
    };
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
    let req = ctx
        .client
        .post(ctx.url(&path))
        .header("content-type", media_type)
        .body(bytes);

    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure(COMMAND, err);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => return failure(COMMAND, err),
            };
            // Current response is {"versionNumber": N}; also preserve the
            // historical numeric response without inventing a missing version.
            let version = data
                .get("versionNumber")
                .unwrap_or(&data)
                .as_u64()
                .filter(|v| *v > 0);
            let Some(version) = version else {
                return failure(COMMAND, crate::output::ErrorBody {
                    code: "version_number_missing",
                    message: "Place publish response has no valid versionNumber".into(),
                    status: None,
                    detail: Some("Inspect place history before repeating this mutation; the request was sent at most once".into()),
                    refs: None,
                });
            };
            let action = if published { "published" } else { "saved" };
            let summary = format!("Place {action} (version {version})");
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

fn place_media_type(file: &std::path::Path) -> Result<&'static str, crate::output::ErrorBody> {
    match file
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("rbxlx") => Ok("application/xml"),
        Some("rbxl") => Ok("application/octet-stream"),
        _ => Err(crate::output::ErrorBody {
            code: "unsupported_place_file",
            message: format!(
                "place files must end in .rbxlx or .rbxl, got {}",
                file.display()
            ),
            status: None,
            detail: Some(".rbxlx is application/xml and .rbxl is application/octet-stream".into()),
            refs: None,
        }),
    }
}

/// Fetch a place's version history.
pub async fn history(ctx: &Ctx, place_id: u64, limit: u32, cursor: Option<&str>) -> Output {
    let path = format!("/place-version-history-api/v1/{place_id}/history");
    let page_size = limit.to_string();
    let mut query = vec![("pageSize", page_size.as_str())];
    if let Some(cursor) = cursor.filter(|cursor| !cursor.is_empty()) {
        query.push(("cursor", cursor));
    }
    match ctx.get_json_query(&path, &query).await {
        Ok(data) => {
            let count = data
                .get("placeVersions")
                .or_else(|| data.get("versions"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let mut out = Output::success(
                "place history",
                format!("Fetched {count} versions for place {place_id}"),
                Some(data.clone()),
            );
            if let Some(cursor) = data.get("nextCursor").and_then(|v| v.as_str())
                && !cursor.is_empty()
            {
                out = out.with_hints([format!("Pass --cursor {cursor} to fetch the next page")]);
            }
            out
        }
        Err(err) => failure("place history", err),
    }
}
