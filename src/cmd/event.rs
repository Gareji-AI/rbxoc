//! Virtual events (in-game seasonal events).
//!
//! `virtual-events` v3 manages game events shown on the experience page.
//! Create and update take provider-defined JSON, so this module forwards a
//! caller-supplied document instead of pinning a schema.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure, failure_for_endpoint};
use crate::output::{ErrorBody, Output};

fn events_path(universe_id: u64) -> String {
    format!("/virtual-events/v3/universes/{universe_id}/game-events")
}

/// List game events under a universe.
pub async fn list(
    ctx: &Ctx,
    universe_id: u64,
    page_size: u32,
    page_token: Option<&str>,
    reverse: bool,
) -> Output {
    let path = events_path(universe_id);
    let page_size_text = page_size.to_string();
    let mut query = vec![("pageSize", page_size_text.as_str())];
    if let Some(token) = page_token.filter(|token| !token.is_empty()) {
        query.push(("pageToken", token));
    }
    if reverse {
        query.push(("reverse", "true"));
    }
    match ctx.get_json_query(&path, &query).await {
        Ok(data) => {
            let count = data
                .get("gameEvents")
                .or_else(|| data.get("events"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let mut out = Output::success(
                "event list",
                format!("Fetched {count} game events"),
                Some(data.clone()),
            );
            if let Some(token) = data.get("nextPageToken").and_then(|value| value.as_str())
                && !token.is_empty()
            {
                out = out.with_hints([format!("Pass --page-token {token} to fetch the next page")]);
            }
            if let Some(token) = data
                .get("previousPageToken")
                .and_then(|value| value.as_str())
                && !token.is_empty()
            {
                out = out.with_hints([format!(
                    "Pass --page-token {token} --reverse to fetch the previous page"
                )]);
            }
            out
        }
        Err(err) => failure_for_endpoint("event list", "event.list", err, &ctx.auth),
    }
}

/// Get one game event.
pub async fn get(ctx: &Ctx, event_id: u64) -> Output {
    let path = format!("/virtual-events/v3/game-events/{event_id}");
    match ctx.get_json(&path).await {
        Ok(data) => Output::success("event get", format!("Fetched event {event_id}"), Some(data)),
        Err(err) => failure_for_endpoint("event get", "event.get", err, &ctx.auth),
    }
}

/// Create a game event from a JSON document.
/// The create document must satisfy the service-required fields.
///
/// `CreateGameEventRequest` documents `title` and `subtitle` as "Clients must
/// provide a non-empty value", requires an `endTime` after `startTime`, and
/// restricts `visibility` to `public`/`private` (`moderated` is server-managed).
/// The server enforces these, so a permissive pass-through is a known-bad request.
fn validate_create_document(body: &Value) -> Result<(), ErrorBody> {
    const REQUIRED: &str =
        "title, subtitle, startTime, endTime, and a visibility of public or private";
    let Some(object) = body.as_object() else {
        return Err(ErrorBody {
            code: "invalid_document",
            message: "--document must be a JSON object".into(),
            status: None,
            detail: Some(format!("A create request requires {REQUIRED}")),
            refs: None,
        });
    };
    for field in ["title", "subtitle"] {
        if object
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ErrorBody {
                code: "invalid_document",
                message: format!("--document needs a non-empty {field}"),
                status: None,
                detail: Some(format!("A create request requires {REQUIRED}")),
                refs: None,
            });
        }
    }
    let timestamp = |field: &str| -> Result<(), ErrorBody> {
        if object.get(field).and_then(Value::as_str).is_none() {
            return Err(ErrorBody {
                code: "invalid_document",
                message: format!("--document needs a {field} timestamp"),
                status: None,
                detail: Some(format!(
                    "Use a timestamp such as 2026-12-01T00:00:00.000Z. A create request requires {REQUIRED}"
                )),
                refs: None,
            });
        }
        Ok(())
    };
    timestamp("startTime")?;
    timestamp("endTime")?;
    let visibility = object.get("visibility").and_then(Value::as_str);
    if let Some(visibility) = visibility
        && !matches!(visibility, "public" | "private")
    {
        return Err(ErrorBody {
            code: "invalid_document",
            message: format!("--document visibility {visibility} is not accepted"),
            status: None,
            detail: Some(
                "Create accepts public or private. moderated is server-managed and rejected."
                    .into(),
            ),
            refs: None,
        });
    }
    Ok(())
}

/// Create a game event from a JSON document.
pub async fn create(ctx: &Ctx, universe_id: u64, document: &str) -> Output {
    let body: Value = match serde_json::from_str(document) {
        Ok(v) => v,
        Err(e) => return invalid_document("event create", e),
    };
    if let Err(err) = validate_create_document(&body) {
        return failure("event create", err);
    }
    let path = events_path(universe_id);
    match ctx.send(ctx.client.post(ctx.url(&path)).json(&body)).await {
        Ok(resp) => {
            finish(
                ctx,
                resp,
                "event create",
                "event.create",
                "Created the game event",
            )
            .await
        }
        Err(err) => failure_for_endpoint("event create", "event.create", err, &ctx.auth),
    }
}

/// Update a game event from a JSON document.
pub async fn update(ctx: &Ctx, event_id: u64, document: &str) -> Output {
    let body: Value = match serde_json::from_str(document) {
        Ok(v) => v,
        Err(e) => return invalid_document("event update", e),
    };
    let path = format!("/virtual-events/v3/game-events/{event_id}");
    match ctx.send(ctx.client.patch(ctx.url(&path)).json(&body)).await {
        Ok(resp) => {
            finish(
                ctx,
                resp,
                "event update",
                "event.update",
                "Updated the game event",
            )
            .await
        }
        Err(err) => failure_for_endpoint("event update", "event.update", err, &ctx.auth),
    }
}

/// Delete a game event.
pub async fn delete(ctx: &Ctx, event_id: u64) -> Output {
    let path = format!("/virtual-events/v3/game-events/{event_id}");
    match ctx.send_empty(ctx.client.delete(ctx.url(&path))).await {
        Ok(_) => Output::success(
            "event delete",
            format!("Deleted event {event_id}"),
            Some(json!({ "eventId": event_id })),
        )
        .with_hints(vec!["Deletes are permanent".into()]),
        Err(err) => failure_for_endpoint("event delete", "event.delete", err, &ctx.auth),
    }
}

fn invalid_document(command: &str, e: serde_json::Error) -> Output {
    failure(
        command,
        ErrorBody {
            code: "invalid_document",
            message: format!("--document is not valid JSON: {e}"),
            status: None,
            detail: Some("The event schema is provider-defined; pass the exact body".into()),

            refs: None,
        },
    )
}

async fn finish(
    ctx: &Ctx,
    resp: reqwest::Response,
    command: &str,
    endpoint_id: &str,
    ok_message: &str,
) -> Output {
    if !resp.status().is_success() {
        let err = crate::auth::error_from_response(resp).await;
        return failure_for_endpoint(command, endpoint_id, err, &ctx.auth);
    }
    let data = match ctx.json_or_error(resp).await {
        Ok(data) => data,
        Err(err) => return failure_for_endpoint(command, endpoint_id, err, &ctx.auth),
    };
    Output::success(command, ok_message.to_string(), Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_path_is_universe_scoped() {
        assert_eq!(
            events_path(42),
            "/virtual-events/v3/universes/42/game-events"
        );
    }

    #[test]
    fn create_document_requires_the_service_fields() {
        // The shipped README example must be accepted.
        let readme = json!({
            "title": "Winter",
            "subtitle": "Seasonal event",
            "startTime": "2026-12-01T00:00:00.000Z",
            "endTime": "2026-12-08T00:00:00.000Z",
            "visibility": "public",
        });
        assert!(validate_create_document(&readme).is_ok(), "{readme}");
        // The incomplete document this replaced must be rejected locally.
        let previous = json!({"title":"Winter","startTime":"2026-12-01T00:00:00Z"});
        assert_eq!(
            validate_create_document(&previous).unwrap_err().code,
            "invalid_document"
        );
        for body in [
            json!({"title":"","subtitle":"s","startTime":"a","endTime":"b"}),
            json!({"title":"t","subtitle":"  ","startTime":"a","endTime":"b"}),
            json!({"title":"t","subtitle":"s","endTime":"b"}),
            json!({"title":"t","subtitle":"s","startTime":"a"}),
            json!({"title":"t","subtitle":"s","startTime":"a","endTime":"b","visibility":"moderated"}),
            json!({"title":"t","subtitle":"s","startTime":"a","endTime":"b","visibility":"Public"}),
            json!([1, 2]),
        ] {
            assert_eq!(
                validate_create_document(&body).unwrap_err().code,
                "invalid_document",
                "{body}"
            );
        }
        // private is accepted too.
        let private = json!({
            "title": "t", "subtitle": "s",
            "startTime": "2026-12-01T00:00:00.000Z", "endTime": "2026-12-08T00:00:00.000Z",
            "visibility": "private",
        });
        assert!(validate_create_document(&private).is_ok());
    }

    #[test]
    fn single_event_path_is_not_universe_scoped() {
        let path = format!("/virtual-events/v3/game-events/{}", 7u64);
        assert_eq!(path, "/virtual-events/v3/game-events/7");
    }
}
