//! Universe operations and messaging.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure, path_segment};
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

const MAX_MESSAGE_CHARS: usize = 1024;

/// The provider field is a string of at most 1,024 characters.
/// A JSON string is unwrapped once so `"hello"` sends `hello`.
/// Every other input, including object-shaped JSON text, is sent unchanged.
fn message_content(message: &str) -> String {
    match serde_json::from_str::<Value>(message) {
        Ok(Value::String(text)) => text,
        _ => message.to_string(),
    }
}

/// Publish a cross-server message.
pub async fn publish_message(ctx: &Ctx, universe_id: u64, topic: &str, message: &str) -> Output {
    let content = message_content(message);
    let chars = content.chars().count();
    if chars > MAX_MESSAGE_CHARS {
        return failure(
            "universe publish-message",
            ErrorBody {
                code: "message_too_large",
                message: format!("--message is {chars} characters; the provider limit is 1,024"),
                status: None,
                detail: Some(
                    "The value is the exact PublishRequest.message string, not a JSON wrapper"
                        .into(),
                ),
                refs: None,
            },
        );
    }
    let encoded_topic = match path_segment(topic) {
        Ok(value) => value,
        Err(err) => return failure("universe publish-message", err),
    };
    let path = format!("/messaging-service/v1/universes/{universe_id}/topics/{encoded_topic}");
    let body = json!({ "message": content });
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
