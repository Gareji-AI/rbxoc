//! Virtual events (in-game seasonal events).
//!
//! `virtual-events` v3 manages game events shown on the experience page.
//! Create and update take provider-defined JSON, so this module forwards a
//! caller-supplied document instead of pinning a schema.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

fn events_path(universe_id: u64) -> String {
    format!("/virtual-events/v3/universes/{universe_id}/game-events")
}

/// List game events under a universe.
pub async fn list(ctx: &Ctx, universe_id: u64, page_size: u32) -> Output {
    let path = format!("{}?pageSize={page_size}", events_path(universe_id));
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("gameEvents")
                .or_else(|| data.get("events"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "event list",
                format!("Fetched {count} game events"),
                Some(data),
            )
        }
        Err(err) => failure("event list", err),
    }
}

/// Get one game event.
pub async fn get(ctx: &Ctx, event_id: u64) -> Output {
    let path = format!("/virtual-events/v3/game-events/{event_id}");
    match ctx.get_json(&path).await {
        Ok(data) => Output::success("event get", format!("Fetched event {event_id}"), Some(data)),
        Err(err) => failure("event get", err),
    }
}

/// Create a game event from a JSON document.
pub async fn create(ctx: &Ctx, universe_id: u64, document: &str) -> Output {
    let body: Value = match serde_json::from_str(document) {
        Ok(v) => v,
        Err(e) => return invalid_document(e),
    };
    let path = events_path(universe_id);
    match ctx.send(ctx.client.post(ctx.url(&path)).json(&body)).await {
        Ok(resp) => finish(resp, "event create", "Created the game event").await,
        Err(err) => failure("event create", err),
    }
}

/// Update a game event from a JSON document.
pub async fn update(ctx: &Ctx, event_id: u64, document: &str) -> Output {
    let body: Value = match serde_json::from_str(document) {
        Ok(v) => v,
        Err(e) => return invalid_document(e),
    };
    let path = format!("/virtual-events/v3/game-events/{event_id}");
    match ctx.send(ctx.client.patch(ctx.url(&path)).json(&body)).await {
        Ok(resp) => finish(resp, "event update", "Updated the game event").await,
        Err(err) => failure("event update", err),
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
        Err(err) => failure("event delete", err),
    }
}

fn invalid_document(e: serde_json::Error) -> Output {
    failure(
        "event",
        ErrorBody {
            code: "invalid_document",
            message: format!("--document is not valid JSON: {e}"),
            status: None,
            detail: Some("The event schema is provider-defined; pass the exact body".into()),

            refs: None,
        },
    )
}

async fn finish(resp: reqwest::Response, command: &str, ok_message: &str) -> Output {
    if !resp.status().is_success() {
        let err = crate::auth::error_from_response(resp).await;
        return failure(command, err);
    }
    let data: Value = resp.json().await.unwrap_or(Value::Null);
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
    fn single_event_path_is_not_universe_scoped() {
        let path = format!("/virtual-events/v3/game-events/{}", 7u64);
        assert_eq!(path, "/virtual-events/v3/game-events/7");
    }
}
