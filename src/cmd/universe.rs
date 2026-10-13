//! Universe operations and messaging.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// Fetch a universe. Also doubles as a connectivity probe.
pub async fn get(ctx: &Ctx, universe_id: u64) -> Output {
    let path = format!("/cloud/v2/universes/{universe_id}");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let name = data
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("(unnamed)");
            Output::success(
                "universe get",
                format!("Fetched universe {universe_id}: {name}"),
                Some(data),
            )
        }
        Err(err) => failure("universe get", err),
    }
}

/// Publish a cross-server message.
pub async fn publish_message(ctx: &Ctx, universe_id: u64, topic: &str, message: &str) -> Output {
    let parsed: Value = match serde_json::from_str(message) {
        Ok(v) => v,
        Err(e) => {
            return failure(
                "universe publish-message",
                ErrorBody {
                    code: "invalid_message",
                    message: format!("--message is not valid JSON: {e}"),
                    status: None,
                    detail: Some("Example: --message '{\"event\":\"restart\"}'".into()),

                    refs: None,
                },
            );
        }
    };
    let path = format!("/messaging-service/v1/universes/{universe_id}/topics/{topic}");
    let body = json!({ "message": parsed.to_string() });
    let req = ctx.client.post(ctx.url(&path)).json(&body);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("universe publish-message", err);
            }
            Output::success(
                "universe publish-message",
                format!("Published to topic {topic}"),
                Some(json!({ "universeId": universe_id, "topic": topic })),
            )
        }
        Err(err) => failure("universe publish-message", err),
    }
}
