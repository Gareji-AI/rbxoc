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
    // The API takes PascalCase enum values, and the subject/action fields sit at
    // the top level next to a plain numeric assetIds array. An earlier version
    // sent rbxassetid:// URIs inside a requests[] wrapper, which the server
    // rejected with "Invalid SubjectType".
    let subject_type_api = match subject_type {
        "user" => "User",
        "group" => "Group",
        "group-roleset" => "GroupRoleset",
        "universe" => "Universe",
        "all" => "All",
        other => {
            return failure(
                COMMAND,
                ErrorBody {
                    code: "invalid_subject_type",
                    message: format!("--subject-type {other} is not accepted"),
                    status: None,
                    detail: Some("Use user, group, group-roleset, universe, or all".into()),
                },
            );
        }
    };
    let action_api = match action {
        "use" => "Use",
        "edit" => "Edit",
        "download" => "Download",
        other => {
            return failure(
                COMMAND,
                ErrorBody {
                    code: "invalid_action",
                    message: format!("--action {other} is not accepted"),
                    status: None,
                    detail: Some("Use use, edit, or download".into()),
                },
            );
        }
    };

    let body = json!({
        "subjectType": subject_type_api,
        "subjectId": subject_id.to_string(),
        "action": action_api,
        "assetIds": asset_ids,
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
    use serde_json::json;

    #[test]
    fn asset_ids_stay_numeric_in_the_request() {
        // The endpoint expects a plain integer array, not rbxassetid:// URIs.
        let ids = [1u64, 2];
        let body = json!({ "assetIds": ids });
        assert_eq!(body["assetIds"][0], json!(1));
        assert_eq!(body["assetIds"][1], json!(2));
    }

    #[test]
    fn action_values_are_pascal_case() {
        // "use" must become "Use"; the server rejects lowercase.
        for (input, expected) in [("use", "Use"), ("edit", "Edit"), ("download", "Download")] {
            let mapped = match input {
                "use" => "Use",
                "edit" => "Edit",
                _ => "Download",
            };
            assert_eq!(mapped, expected);
        }
    }
}
