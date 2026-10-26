//! Place publishing and retrieval.
//!
//! `POST /universes/v1/{universeId}/places/{placeId}/versions` sends an RBXL /
//! RBXLX file as `data-binary`. `versionType` of `Saved` only saves;
//! `Published` also goes live.
//!
//! The response is the new place version number. A malformed file yields HTTP 400.

use serde_json::json;

use crate::ctx::{Ctx, failure, failure_for_endpoint};
use crate::output::Output;

const COMMAND: &str = "place publish";

/// `x-roblox-size-limit` on the official place-version operation.
pub const PLACE_PUBLISH_LIMIT_BYTES: u64 = 10 * 1024 * 1024;

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
    // Reject an oversized file before reading it, so the CLI never builds a
    // request the provider documents as too large.
    if let Err(err) = check_place_file_size(file).await {
        return failure(COMMAND, err);
    }
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
                return failure_for_endpoint(COMMAND, "place.publish", err, &ctx.auth);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => {
                    return failure_for_endpoint(COMMAND, "place.publish", err, &ctx.auth);
                }
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
        Err(err) => failure_for_endpoint(COMMAND, "place.publish", err, &ctx.auth),
    }
}

/// Enforce the documented `x-roblox-size-limit` locally so an oversized file
/// never reaches the network. A file exactly at the limit is still eligible.
async fn check_place_file_size(file: &std::path::Path) -> Result<(), crate::output::ErrorBody> {
    let size = match tokio::fs::metadata(file).await {
        Ok(metadata) => metadata.len(),
        Err(err) => {
            return Err(crate::output::ErrorBody {
                code: "file_unreadable",
                message: format!("cannot read the file: {}", file.display()),
                status: None,
                detail: Some(err.to_string()),
                refs: None,
            });
        }
    };
    if size <= PLACE_PUBLISH_LIMIT_BYTES {
        return Ok(());
    }
    Err(crate::output::ErrorBody {
        code: "place_file_too_large",
        message: format!(
            "Place file is {size} bytes, above the documented {PLACE_PUBLISH_LIMIT_BYTES} byte limit"
        ),
        status: None,
        detail: Some(format!(
            "{} is {size} bytes; the Open Cloud place-version operation declares x-roblox-size-limit {PLACE_PUBLISH_LIMIT_BYTES} (10 MiB)",
            file.display()
        )),
        refs: None,
    })
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
        Err(err) => failure_for_endpoint("place get", "place.get", err, &ctx.auth),
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

/// Change a place's name, description, or server size.
///
/// The Cloud v2 `Universe.displayName` and `Universe.description` are
/// read-only; the writable fields live on `Place`, so this is a
/// `Cloud_UpdatePlace` PATCH with an explicit `updateMask` built only from the
/// flags the caller supplied. Omitted fields are never cleared.
pub async fn update(
    ctx: &Ctx,
    universe_id: u64,
    place_id: u64,
    display_name: Option<&str>,
    description: Option<&str>,
    server_size: Option<u32>,
) -> Output {
    update_with_command(
        ctx,
        "place update",
        universe_id,
        place_id,
        display_name,
        description,
        server_size,
    )
    .await
}

/// Dry-run planning for `place update`. Needs no credentials and sends nothing.
pub fn preview_update(
    universe_id: u64,
    place_id: u64,
    display_name: Option<&str>,
    description: Option<&str>,
    server_size: Option<u32>,
) -> Output {
    match plan_update(display_name, description, server_size) {
        Ok((mask, body)) => Output::success(
            "place update (dry-run)",
            format!("Would update place {place_id} fields: {mask}"),
            Some(json!({
                "universeId": universe_id,
                "placeId": place_id,
                "updateMask": mask,
                "body": body,
                "dryRun": true,
            })),
        )
        .with_hints(vec![
            "No cloud call was made and no credentials were read".into(),
            "Re-run without --dry-run to apply the update".into(),
        ]),
        Err(err) => failure("place update (dry-run)", err),
    }
}

async fn update_with_command(
    ctx: &Ctx,
    command: &str,
    universe_id: u64,
    place_id: u64,
    display_name: Option<&str>,
    description: Option<&str>,
    server_size: Option<u32>,
) -> Output {
    let (mask, body) = match plan_update(display_name, description, server_size) {
        Ok(planned) => planned,
        Err(err) => return failure(command, err),
    };
    let path = format!("/cloud/v2/universes/{universe_id}/places/{place_id}");
    let req = ctx
        .client
        .patch(ctx.url(&path))
        .query(&[("updateMask", mask.as_str())])
        .json(&body);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure_for_endpoint(command, "place.update", err, &ctx.auth);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => {
                    return failure_for_endpoint(command, "place.update", err, &ctx.auth);
                }
            };
            // The provider returns the updated Place resource. Validate that it
            // is an object so a malformed success cannot be reported as done.
            if !data.is_object() {
                return failure(
                    command,
                    crate::output::ErrorBody {
                        code: "place_response_invalid",
                        message: "place update response is not a Place resource".into(),
                        status: None,
                        detail: Some(
                            "Inspect the place with `rbxoc place get` before repeating this mutation"
                                .into(),
                        ),
                        refs: None,
                    },
                );
            }
            for (field, expected) in [
                ("displayName", display_name.map(str::to_string)),
                ("description", description.map(str::to_string)),
            ] {
                if let Some(expected) = expected
                    && data.get(field).and_then(|value| value.as_str()) != Some(expected.as_str())
                {
                    return place_response_mismatch(command, field);
                }
            }
            if let Some(expected) = server_size {
                let actual = data.get("serverSize").and_then(|value| value.as_u64());
                if actual != Some(u64::from(expected)) {
                    return place_response_mismatch(command, "serverSize");
                }
            }
            let updated = data
                .get("displayName")
                .and_then(|value| value.as_str())
                .unwrap_or("(unnamed)");
            let mut out = Output::success(
                command,
                format!("Updated place: {updated} (fields: {mask})"),
                Some(data),
            );
            out = out.with_hints(vec![
                "The root place's displayName and description are what `universe get` reports"
                    .into(),
            ]);
            out
        }
        Err(err) => failure_for_endpoint(command, "place.update", err, &ctx.auth),
    }
}

/// Build the `updateMask` and body from the supplied fields only.
fn plan_update(
    display_name: Option<&str>,
    description: Option<&str>,
    server_size: Option<u32>,
) -> Result<(String, serde_json::Value), crate::output::ErrorBody> {
    let mut mask = Vec::new();
    let mut body = serde_json::Map::new();
    if let Some(name) = display_name {
        if name.trim().is_empty() {
            return Err(invalid_update("--display-name must not be blank"));
        }
        mask.push("displayName");
        body.insert("displayName".into(), json!(name));
    }
    if let Some(text) = description {
        mask.push("description");
        body.insert("description".into(), json!(text));
    }
    if let Some(size) = server_size {
        if size == 0 {
            return Err(invalid_update("--server-size must be at least 1"));
        }
        mask.push("serverSize");
        body.insert("serverSize".into(), json!(size));
    }
    if mask.is_empty() {
        return Err(invalid_update(
            "Pass at least one of --display-name, --description, --description-file or --server-size",
        ));
    }
    Ok((mask.join(","), serde_json::Value::Object(body)))
}

fn place_response_mismatch(command: &str, field: &str) -> Output {
    failure(
        command,
        crate::output::ErrorBody {
            code: "place_response_mismatch",
            message: format!("place update response does not report the new {field}"),
            status: None,
            detail: Some(
                "Inspect the place with `rbxoc place get` before repeating this mutation".into(),
            ),
            refs: None,
        },
    )
}

fn invalid_update(message: &str) -> crate::output::ErrorBody {
    crate::output::ErrorBody {
        code: "invalid_value",
        message: message.into(),
        status: None,
        detail: Some(
            "`place update` sends only the fields you pass and never clears the others".into(),
        ),
        refs: None,
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
        Err(err) => failure_for_endpoint("place history", "place.history", err, &ctx.auth),
    }
}
