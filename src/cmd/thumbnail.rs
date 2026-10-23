//! Thumbnail replacement.
//!
//! `thumbnail-personalization-api` is experimental. Expect the schema to
//! change without notice. Needs an API key
//! (`universe.thumbnail:write`).
//!
//! Uploads are asynchronous. Pass the returned operationIds to
//! `thumbnail status`.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

const COMMAND: &str = "thumbnail upload";

/// Replace homepage thumbnails.
pub async fn upload(ctx: &Ctx, universe_id: u64, files: &[std::path::PathBuf]) -> Output {
    if files.is_empty() {
        return failure(
            COMMAND,
            ErrorBody {
                code: "no_files",
                message: "no images to upload".into(),
                status: None,
                detail: None,

                refs: None,
            },
        );
    }

    let mut form = reqwest::multipart::Form::new();
    for file in files {
        let bytes = match tokio::fs::read(file).await {
            Ok(b) => b,
            Err(e) => {
                return failure(
                    COMMAND,
                    ErrorBody {
                        code: "file_unreadable",
                        message: format!("cannot read the image: {}", file.display()),
                        status: None,
                        detail: Some(e.to_string()),

                        refs: None,
                    },
                );
            }
        };
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("thumb.png")
            .to_string();
        let lower = name.to_ascii_lowercase();
        let ct = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            "image/jpeg"
        } else {
            "image/png"
        };
        let part = match reqwest::multipart::Part::bytes(bytes)
            .file_name(name)
            .mime_str(ct)
        {
            Ok(p) => p,
            Err(e) => {
                return failure(
                    COMMAND,
                    ErrorBody {
                        code: "mime_invalid",
                        message: format!("invalid content-type: {e}"),
                        status: None,
                        detail: None,

                        refs: None,
                    },
                );
            }
        };
        // Form has no push API; take it and rebuild.
        let taken = std::mem::replace(&mut form, reqwest::multipart::Form::new());
        form = taken.part("files", part);
    }

    let path =
        format!("/thumbnail-personalization-api/v1/universes/{universe_id}/thumbnails/uploads");
    let req = ctx.client.post(ctx.url(&path)).multipart(form);

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
            let ops = extract_operation_ids(&data);
            if ops.is_empty() {
                return failure(COMMAND, ErrorBody {
                    code: "operation_id_missing",
                    message: "Thumbnail upload response contains no usable operation IDs".into(),
                    status: None,
                    detail: Some("Inspect existing thumbnails and the response before repeating this upload".into()),
                    refs: None,
                });
            }
            let mut out = Output::success(
                COMMAND,
                format!(
                    "Accepted {} thumbnails ({} operations)",
                    files.len(),
                    ops.len()
                ),
                Some(json!({ "operationIds": ops, "response": data })),
            );
            if let Some(first) = ops.first() {
                out = out.with_hints(vec![
                    format!(
                        "Run `rbxoc thumbnail status --universe-id {universe_id} --operation-ids {first}` to check progress"
                    ),
                    "Processing is asynchronous; it takes time to appear".into(),
                    "This API is experimental; the schema may change".into(),
                ]);
            }
            out
        }
        Err(err) => failure(COMMAND, err),
    }
}

/// Pull operation IDs out of the response. The shape is experimental, so
/// several locations are checked.
fn extract_operation_ids(data: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(map) = data.get("fileToOperationIdDict").and_then(Value::as_object) {
        for value in map.values().filter_map(Value::as_str) {
            if let Some(id) = usable_operation_id(value) {
                ids.push(id);
            }
        }
    }
    for key in ["operationIds", "operations", "paths"] {
        if let Some(arr) = data.get(key).and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(raw) = item
                    .as_str()
                    .or_else(|| item.get("path").and_then(Value::as_str))
                    && let Some(id) = usable_operation_id(raw)
                {
                    ids.push(id);
                }
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

fn usable_operation_id(raw: &str) -> Option<String> {
    let id = raw.strip_prefix("operations/").unwrap_or(raw);
    (!id.trim().is_empty()).then(|| id.to_string())
}

/// Check upload processing status.
pub async fn status(ctx: &Ctx, universe_id: u64, operation_ids: &[String]) -> Output {
    if operation_ids.is_empty() {
        return failure(
            "thumbnail status",
            ErrorBody {
                code: "no_operation_ids",
                message: "operationIds are required".into(),
                status: None,
                detail: None,

                refs: None,
            },
        );
    }
    let query: Vec<_> = operation_ids
        .iter()
        .map(|id| ("operationIds", id.as_str()))
        .collect();
    let path = format!(
        "/thumbnail-personalization-api/v1/universes/{universe_id}/thumbnails/uploads/status"
    );
    match ctx.get_json_query(&path, &query).await {
        Ok(data) => Output::success(
            "thumbnail status",
            format!("Fetched status for {} operations", operation_ids.len()),
            Some(data),
        ),
        Err(err) => failure("thumbnail status", err),
    }
}

/// List homepage thumbnails.
pub async fn list(ctx: &Ctx, universe_id: u64, next_cursor: Option<&str>) -> Output {
    let path = format!("/thumbnail-personalization-api/v1/universes/{universe_id}/thumbnails");
    let data = if let Some(cursor) = next_cursor.filter(|cursor| !cursor.is_empty()) {
        ctx.get_json_query(&path, &[("nextCursor", cursor)]).await
    } else {
        ctx.get_json(&path).await
    };
    match data {
        Ok(data) => {
            let mut out = Output::success(
                "thumbnail list",
                format!("Fetched thumbnails for universe {universe_id}"),
                Some(data.clone()),
            );
            if let Some(cursor) = data.get("nextCursor").and_then(|value| value.as_str())
                && !cursor.is_empty()
            {
                out = out.with_hints([format!(
                    "Pass --next-cursor {cursor} to fetch the next page"
                )]);
            }
            out
        }
        Err(err) => failure("thumbnail list", err),
    }
}

/// Delete thumbnails.
pub async fn delete(ctx: &Ctx, universe_id: u64, thumbnail_ids: &[String]) -> Output {
    if thumbnail_ids.is_empty() || thumbnail_ids.iter().any(|id| id.trim().is_empty()) {
        return failure(
            "thumbnail delete",
            ErrorBody {
                code: "invalid_thumbnail_ids",
                message: "--thumbnail-ids requires at least one non-empty ID".into(),
                status: None,
                detail: Some("Pass comma-separated IDs from `rbxoc thumbnail list`".into()),
                refs: None,
            },
        );
    }
    let path = format!("/thumbnail-personalization-api/v1/universes/{universe_id}/thumbnails");
    // OpenAPI query arrays default to form/explode: repeat the key per ID.
    let query: Vec<_> = thumbnail_ids
        .iter()
        .map(|id| ("homepageThumbnailIds", id.as_str()))
        .collect();
    let req = ctx.client.delete(ctx.url(&path)).query(&query);
    let resp = match ctx.send(req).await {
        Ok(resp) => resp,
        Err(err) => return failure("thumbnail delete", err),
    };
    if !resp.status().is_success() {
        let err = crate::auth::error_from_response(resp).await;
        return failure("thumbnail delete", err);
    }
    let data = match ctx.json_or_error(resp).await {
        Ok(data) => data,
        Err(err) => return failure("thumbnail delete", err),
    };
    summarize_delete(universe_id, thumbnail_ids, &data)
}

/// The documented 200 body maps each requested thumbnail ID to a boolean.
/// Only an explicit `true` counts as deleted, so a partial result can never
/// look like an unqualified success.
fn summarize_delete(universe_id: u64, requested: &[String], data: &Value) -> Output {
    let Some(results) = data.as_object() else {
        // The body is unusable, but the caller still needs to know the request
        // was sent and what came back.
        let mut out = failure(
            "thumbnail delete",
            ErrorBody {
                code: "thumbnail_response_invalid",
                message: "thumbnail delete response is not a JSON object".into(),
                status: None,
                detail: Some(
                    "Inspect `rbxoc thumbnail list` before repeating this mutation; the request was sent at most once"
                        .into(),
                ),
                refs: None,
            },
        );
        out.data = Some(json!({
            "universeId": universe_id,
            "requested": requested,
            "response": data,
        }));
        return out;
    };
    let mut deleted = Vec::new();
    let mut not_deleted = Vec::new();
    let mut unconfirmed = Vec::new();
    for id in requested {
        match results.get(id) {
            Some(Value::Bool(true)) => deleted.push(id.clone()),
            Some(Value::Bool(false)) => not_deleted.push(id.clone()),
            // Absent or non-boolean entries cannot prove anything was removed.
            _ => unconfirmed.push(id.clone()),
        }
    }
    let data = json!({
        "universeId": universe_id,
        "requested": requested,
        "deleted": deleted,
        "notDeleted": not_deleted,
        "unconfirmed": unconfirmed,
        "removed": deleted.len(),
        "partial": !not_deleted.is_empty() || !unconfirmed.is_empty(),
        "results": results,
    });
    if not_deleted.is_empty() && unconfirmed.is_empty() {
        return Output::success(
            "thumbnail delete",
            format!("Deleted {} thumbnails", deleted.len()),
            Some(data),
        );
    }
    if deleted.is_empty() {
        // Keep the provider's per-ID result: this is a provider-confirmed but
        // unresolved mutation, so the caller needs the receipt to reconcile.
        let mut out = failure(
            "thumbnail delete",
            ErrorBody {
                code: "thumbnail_delete_failed",
                message: format!(
                    "No requested thumbnail was confirmed deleted ({} not deleted, {} unconfirmed)",
                    not_deleted.len(),
                    unconfirmed.len()
                ),
                status: None,
                detail: Some(
                    "The provider answered 200 but reported no successful deletion".into(),
                ),
                refs: None,
            },
        );
        out.data = Some(data);
        return out;
    }
    Output::success(
        "thumbnail delete",
        format!(
            "Deleted {} of {} thumbnails; the result is partial",
            deleted.len(),
            requested.len()
        ),
        Some(data),
    )
    .with_hints([
        "Read `data.notDeleted` and `data.unconfirmed` and reconcile them before retrying".into(),
        "Run `rbxoc thumbnail list` to confirm which thumbnails still exist".into(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_operation_ids_from_flat_array() {
        let data = json!({ "operationIds": ["abc", "def"] });
        assert_eq!(extract_operation_ids(&data), vec!["abc", "def"]);
    }

    #[test]
    fn extracts_and_strips_prefix() {
        let data = json!({ "operations": [{ "path": "operations/xyz" }] });
        assert_eq!(extract_operation_ids(&data), vec!["xyz"]);
    }

    #[test]
    fn dedupes_and_ignores_missing() {
        let data = json!({ "operationIds": ["a", "a"], "other": 1 });
        assert_eq!(extract_operation_ids(&data), vec!["a"]);
        assert!(extract_operation_ids(&json!({})).is_empty());
    }
}
