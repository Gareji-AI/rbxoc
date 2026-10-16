//! Badge management.
//!
//! Badges are created and updated through the `legacy-badges` family. The
//! name is historical; these are the current documented endpoints.
//!
//! Caution: creating a badge spends Robux, and the OAuth2 scope is literally
//! `legacy-universe.badge:manage-and-spend-robux`. The catalog marks this so
//! an agent asks before acting.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// Create a badge under a universe.
pub async fn create(
    ctx: &Ctx,
    universe_id: u64,
    name: &str,
    description: &str,
    payment_source: Option<&str>,
) -> Output {
    const COMMAND: &str = "badge create";
    // Roblox requires knowing who pays the creation fee.
    let payment_source_type = match payment_source {
        Some("user") => "1",
        Some("group") => "2",
        Some(_) => {
            return failure(
                COMMAND,
                ErrorBody {
                    code: "invalid_payment_source",
                    message: "--payment-source must be user or group".into(),
                    status: None,
                    detail: Some(
                        "Creating a badge spends Robux; confirm who pays before submitting".into(),
                    ),
                    refs: None,
                },
            );
        }
        None => {
            return failure(
                COMMAND,
                ErrorBody {
                    code: "payment_source_missing",
                    message: "--payment-source is required to create a badge".into(),
                    status: None,
                    detail: Some(
                        "Roblox charges Robux to create a badge. Pass --payment-source user or group.".into(),
                    ),

                    refs: None,
                },
            );
        }
    };

    let path = format!("/legacy-badges/v1/universes/{universe_id}/badges");
    let form = reqwest::multipart::Form::new()
        .text("name", name.to_string())
        .text("description", description.to_string())
        .text("paymentSourceType", payment_source_type);
    let req = ctx.client.post(ctx.url(&path)).multipart(form);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure(COMMAND, err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            Output::success(COMMAND, format!("Created badge {name}"), Some(data))
                .with_hints(vec!["Creating a badge spends Robux".into()])
        }
        Err(err) => failure(COMMAND, err),
    }
}

/// Update a badge's configuration.
pub async fn update(
    ctx: &Ctx,
    badge_id: u64,
    name: Option<&str>,
    description: Option<&str>,
    enabled: Option<bool>,
) -> Output {
    const COMMAND: &str = "badge update";
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(d) = description {
        body["description"] = json!(d);
    }
    if let Some(e) = enabled {
        body["enabled"] = json!(e);
    }
    if body.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return failure(
            COMMAND,
            ErrorBody {
                code: "nothing_to_update",
                message: "no fields to update".into(),
                status: None,
                detail: Some("Pass --name, --description, or --enabled".into()),

                refs: None,
            },
        );
    }

    let path = format!("/legacy-badges/v1/badges/{badge_id}");
    let req = ctx.client.patch(ctx.url(&path)).json(&body);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure(COMMAND, err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            Output::success(COMMAND, format!("Updated badge {badge_id}"), Some(data))
        }
        Err(err) => failure(COMMAND, err),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn badge_create_path_is_universe_scoped() {
        let path = format!("/legacy-badges/v1/universes/{}/badges", 123u64);
        assert_eq!(path, "/legacy-badges/v1/universes/123/badges");
    }

    #[test]
    fn badge_update_path_is_badge_scoped() {
        let path = format!("/legacy-badges/v1/badges/{}", 456u64);
        assert_eq!(path, "/legacy-badges/v1/badges/456");
    }
}
