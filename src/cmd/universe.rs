//! universe 操作とメッセージング。

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// universe の情報を取得する。認証の疎通確認にも使う。
pub async fn get(ctx: &Ctx, universe_id: u64) -> Output {
    let path = format!("/cloud/v2/universes/{universe_id}");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let name = data
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("(名称なし)");
            Output::success(
                "universe get",
                format!("universe {universe_id} を取得しました: {name}"),
                Some(data),
            )
        }
        Err(err) => failure("universe get", err),
    }
}

/// クロスサーバー メッセージを配信する。
pub async fn publish_message(ctx: &Ctx, universe_id: u64, topic: &str, message: &str) -> Output {
    let parsed: Value = match serde_json::from_str(message) {
        Ok(v) => v,
        Err(e) => {
            return failure(
                "universe publish-message",
                ErrorBody {
                    code: "invalid_message",
                    message: format!("--message が JSON として解釈できません: {e}"),
                    status: None,
                    detail: Some("例: --message '{\"event\":\"restart\"}'".into()),
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
                format!("topic {topic} へ配信しました"),
                Some(json!({ "universeId": universe_id, "topic": topic })),
            )
        }
        Err(err) => failure("universe publish-message", err),
    }
}
