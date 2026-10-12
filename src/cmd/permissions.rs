//! Asset permission management.
//!
//! Owns one endpoint: `PATCH /asset-permissions-api/v1/assets/permissions`.
//! Used to grant a user or group access to assets, which matters when a group
//! collaborates on a place and the assets live under one account.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// Grant a subject access to a set of assets.
pub async fn grant(
    ctx: &Ctx,
    asset_ids: &[u64],
    subject_type: &str,
    subject_id: u64,
    action: &str,
) -> Output {
    const COMMAND: &str = "permissions grant";

    if asset_ids.is_empty() {
        return failure(
            COMMAND,
            ErrorBody {
                code: "no_assets",
                message: "at least one --asset-id is required".into(),
                status: None,
                detail: None,
            },
        );
    }
    if !matches!(subject_type, "user" | "group") {
        return failure(
            COMMAND,
            ErrorBody {
                code: "invalid_subject_type",
                message: format!("--subject-type must be user or group, got {subject_type}"),
                status: None,
                detail: None,
            },
        );
    }
    if !matches!(action, "use" | "edit") {
        return failure(
            COMMAND,
            ErrorBody {
                code: "invalid_action",
                message: format!("--action must be use or edit, got {action}"),
                status: None,
                detail: Some(
                    "Use \"use\" to let the subject load the asset, \"edit\" to modify it".into(),
                ),
            },
        );
    }

    let subject = match subject_type {
        "user" => json!({ "userId": subject_id.to_string() }),
        _ => json!({ "groupId": subject_id.to_string() }),
    };
    let asset_uris: Vec<String> = asset_ids
        .iter()
        .map(|id| format!("rbxassetid://{id}"))
        .collect();
    let body = json!({
        "requests": [{
            "assetUris": asset_uris,
            "subject": subject,
            "action": action,
        }]
    });

    let req = ctx
        .client
        .patch(ctx.url("/asset-permissions-api/v1/assets/permissions"))
        .json(&body);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure(COMMAND, err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            Output::success(
                COMMAND,
                format!(
                    "Granted {action} on {} asset(s) to {subject_type} {subject_id}",
                    asset_ids.len()
                ),
                Some(json!({
                    "assetIds": asset_ids,
                    "subjectType": subject_type,
                    "subjectId": subject_id,
                    "action": action,
                    "response": data,
                })),
            )
        }
        Err(err) => failure(COMMAND, err),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn asset_uris_use_the_rbxassetid_scheme() {
        let ids = [1u64, 2];
        let uris: Vec<String> = ids.iter().map(|id| format!("rbxassetid://{id}")).collect();
        assert_eq!(uris, vec!["rbxassetid://1", "rbxassetid://2"]);
    }
}
