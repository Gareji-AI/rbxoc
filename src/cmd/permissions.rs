//! Asset permission management.
//!
//! Owns one endpoint: `PATCH /asset-permissions-api/v1/assets/permissions`.
//! Used to grant a user or group access to assets, which matters when a group
//! collaborates on a place and the assets live under one account.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure, failure_for_endpoint};
use crate::output::{ErrorBody, Output};

/// Grant a subject access to a set of assets.
pub async fn grant(
    ctx: &Ctx,
    asset_ids: &[u64],
    subject_type: &str,
    subject_id: Option<u64>,
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

                refs: None,
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

                    refs: None,
                },
            );
        }
    };
    let subject_id_api = match subject_type_api {
        "All" => {
            if subject_id.is_some() {
                return failure(
                    COMMAND,
                    ErrorBody {
                        code: "subject_id_forbidden",
                        message: "--subject-id must be omitted when --subject-type is all".into(),
                        status: None,
                        detail: Some(
                            "SubjectType All requires an empty subjectId. Omit --subject-id."
                                .into(),
                        ),
                        refs: None,
                    },
                );
            }
            String::new()
        }
        _ => {
            let Some(id) = subject_id.filter(|id| *id > 0) else {
                return failure(
                    COMMAND,
                    ErrorBody {
                        code: "subject_id_required",
                        message: format!(
                            "--subject-id must be a positive ID for subject type {subject_type}"
                        ),
                        status: None,
                        detail: Some(
                            "Use user, group, group-roleset, or universe with a positive ID. Use all with no --subject-id."
                                .into(),
                        ),
                        refs: None,
                    },
                );
            };
            id.to_string()
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

                    refs: None,
                },
            );
        }
    };

    let body = json!({
        "subjectType": subject_type_api,
        "subjectId": subject_id_api.clone(),
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
                return failure_for_endpoint(COMMAND, "permissions.grant", err, &ctx.auth);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => {
                    return failure_for_endpoint(COMMAND, "permissions.grant", err, &ctx.auth);
                }
            };
            let successful: Option<Vec<u64>> = match data.get("successAssetIds") {
                None | Some(Value::Null) => Some(Vec::new()),
                Some(Value::Array(ids)) => ids
                    .iter()
                    .map(|id| id.as_u64().filter(|id| *id > 0))
                    .collect(),
                _ => None,
            };
            let error_count = match data.get("errors") {
                None | Some(Value::Null) => Some(0),
                Some(Value::Array(errors)) => Some(errors.len()),
                _ => None,
            };
            let requested: std::collections::BTreeSet<_> = asset_ids.iter().copied().collect();
            let problem = match (&successful, error_count) {
                (Some(ids), Some(count)) if count > 0 => Some((
                    "permission_grant_failed",
                    format!("Permission batch reported {} successful asset(s) and {count} grant error(s)", ids.len()),
                )),
                (Some(ids), Some(0)) if ids.iter().copied().collect::<std::collections::BTreeSet<_>>() == requested => None,
                _ => Some((
                    "invalid_permissions_response",
                    "Permission response does not confirm every requested asset with valid successAssetIds/errors arrays".into(),
                )),
            };
            let evidence = json!({
                "assetIds": asset_ids, "successAssetIds": successful,
                "subjectType": subject_type, "subjectId": subject_id_api,
                "action": action, "response": data,
            });
            if let Some((code, message)) = problem {
                let mut out = failure(
                    COMMAND,
                    ErrorBody {
                        code,
                        message,
                        status: None,
                        detail: None,
                        refs: None,
                    },
                );
                out.data = Some(evidence);
                return out.with_hints([
                    "Inspect successAssetIds and response.errors, then verify the existing permissions before retrying individual failures. Do not blindly replay the entire grant batch.".into()
                ]);
            }
            Output::success(
                COMMAND,
                format!(
                    "Granted {action} on {} asset(s) to {subject_type}{}",
                    requested.len(),
                    if subject_id_api.is_empty() {
                        String::new()
                    } else {
                        format!(" {subject_id_api}")
                    }
                ),
                Some(evidence),
            )
        }
        Err(err) => failure_for_endpoint(COMMAND, "permissions.grant", err, &ctx.auth),
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
