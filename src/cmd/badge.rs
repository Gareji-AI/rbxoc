//! Badge management.
//!
//! Badges are created and updated through the `legacy-badges` family. The
//! name is historical; these are the current documented endpoints.
//!
//! Caution: creating a badge spends Robux, and the OAuth2 scope is literally
//! `legacy-universe.badge:manage-and-spend-robux`. The catalog marks this so
//! an agent asks before acting.

use serde_json::json;

use crate::ctx::{Ctx, failure, failure_for_endpoint};
use crate::output::{ErrorBody, Output};

/// Inputs for a paid badge creation. Cost and active state are required so a
/// request is never sent without an explicit Robux confirmation.
pub struct BadgeCreate<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub payment_source: Option<&'a str>,
    pub expected_cost: Option<u64>,
    pub active: Option<bool>,
    pub icon: Option<&'a std::path::Path>,
}

/// Create a badge under a universe.
pub async fn create(ctx: &Ctx, universe_id: u64, request: BadgeCreate<'_>) -> Output {
    const COMMAND: &str = "badge create";
    // Roblox requires knowing who pays the creation fee.
    let payment_source_type = match request.payment_source {
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
    let Some(expected_cost) = request.expected_cost else {
        return failure(
            COMMAND,
            ErrorBody {
                code: "expected_cost_missing",
                message: "--expected-cost is required before a paid badge request".into(),
                status: None,
                detail: Some(
                    "Pass the Robux amount you expect this creation to cost. Nothing is sent until that confirmation is present."
                        .into(),
                ),
                refs: None,
            },
        );
    };
    let Some(active) = request.active else {
        return failure(
            COMMAND,
            ErrorBody {
                code: "active_missing",
                message: "--active is required and must be true or false".into(),
                status: None,
                detail: Some(
                    "The badge contract sends isActive explicitly. There is no hidden default."
                        .into(),
                ),
                refs: None,
            },
        );
    };
    let icon = if let Some(path) = request.icon {
        match badge_icon_part(path).await {
            Ok(part) => Some(part),
            Err(out) => return *out,
        }
    } else {
        None
    };

    let path = format!("/legacy-badges/v1/universes/{universe_id}/badges");
    let mut form = reqwest::multipart::Form::new()
        .text("name", request.name.to_string())
        .text("description", request.description.to_string())
        .text("paymentSourceType", payment_source_type)
        .text("expectedCost", expected_cost.to_string())
        .text("isActive", active.to_string());
    if let Some(icon) = icon {
        form = form.part("files", icon);
    }
    let req = ctx.client.post(ctx.url(&path)).multipart(form);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure_for_endpoint(COMMAND, "badge.create", err, &ctx.auth);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => return failure_for_endpoint(COMMAND, "badge.create", err, &ctx.auth),
            };
            Output::success(
                COMMAND,
                format!("Created badge {}", request.name),
                Some(data),
            )
            .with_hints(vec![
                format!(
                    "Creating a badge spends Robux; expectedCost {expected_cost} was confirmed"
                ),
                format!("isActive was sent as {active}"),
                "files is omitted only when the universe has a curated default badge icon".into(),
            ])
        }
        Err(err) => failure_for_endpoint(COMMAND, "badge.create", err, &ctx.auth),
    }
}

async fn badge_icon_part(path: &std::path::Path) -> Result<reqwest::multipart::Part, Box<Output>> {
    const MAX_BYTES: u64 = 10 * 1024 * 1024;
    let fail = |code: &'static str, message: String, detail: Option<String>| {
        Box::new(failure(
            "badge create",
            ErrorBody {
                code,
                message,
                status: None,
                detail,
                refs: None,
            },
        ))
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            fail(
                "invalid_badge_icon",
                format!(
                    "badge icon needs a .png, .jpg, or .jpeg file name ({})",
                    path.display()
                ),
                None,
            )
        })?;
    let lower = name.to_ascii_lowercase();
    let content_type = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        return Err(fail(
            "invalid_badge_icon",
            format!(
                "badge icon must be .png, .jpg, or .jpeg, got {}",
                path.display()
            ),
            Some("The files field is sent only after local type validation.".into()),
        ));
    };
    let metadata = tokio::fs::metadata(path).await.map_err(|err| {
        fail(
            "file_unreadable",
            format!("cannot read the badge icon: {}", path.display()),
            Some(err.to_string()),
        )
    })?;
    if !metadata.is_file() {
        return Err(fail(
            "invalid_badge_icon",
            format!("badge icon is not a regular file: {}", path.display()),
            None,
        ));
    }
    let size = metadata.len();
    if size == 0 || size > MAX_BYTES {
        return Err(fail(
            "file_size_invalid",
            format!(
                "badge icon must be nonempty and at most {MAX_BYTES} bytes, got {size} ({})",
                path.display()
            ),
            None,
        ));
    }
    let bytes = tokio::fs::read(path).await.map_err(|err| {
        fail(
            "file_unreadable",
            format!("cannot read the badge icon: {}", path.display()),
            Some(err.to_string()),
        )
    })?;
    reqwest::multipart::Part::bytes(bytes)
        .file_name(name.to_string())
        .mime_str(content_type)
        .map_err(|err| {
            fail(
                "mime_invalid",
                format!("invalid badge icon content-type: {err}"),
                None,
            )
        })
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
                return failure_for_endpoint(COMMAND, "badge.update", err, &ctx.auth);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => return failure_for_endpoint(COMMAND, "badge.update", err, &ctx.auth),
            };
            Output::success(COMMAND, format!("Updated badge {badge_id}"), Some(data))
        }
        Err(err) => failure_for_endpoint(COMMAND, "badge.update", err, &ctx.auth),
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
