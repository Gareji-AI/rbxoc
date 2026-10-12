//! Monetization (Game Passes / Developer Products).
//!
//! Open Cloud took these over on 2025-12-04. Images ride in the same multipart
//! request. Still labeled Beta in the official docs.
//!
//! API keys scope permissions per universe.

use serde_json::Value;

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// List Game Passes.
pub async fn gamepass_list(ctx: &Ctx, universe_id: u64) -> Output {
    let path = format!("/game-passes/v1/universes/{universe_id}/game-passes/creator");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("gamePasses")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "gamepass list",
                format!("Fetched {count} Game Passes"),
                Some(data),
            )
        }
        Err(err) => failure("gamepass list", err),
    }
}

/// Create a Game Pass.
#[allow(clippy::too_many_arguments)]
pub async fn gamepass_create(
    ctx: &Ctx,
    universe_id: u64,
    name: &str,
    description: Option<&str>,
    price: Option<u64>,
    image: Option<&std::path::PathBuf>,
) -> Output {
    // Each property is its own multipart part; there is no `request` JSON field.
    let mut fields: Vec<(&str, String)> = vec![("name", name.to_string())];
    if let Some(d) = description {
        fields.push(("description", d.to_string()));
    }
    if let Some(p) = price {
        fields.push(("price", p.to_string()));
        fields.push(("isForSale", "true".to_string()));
    }
    let path = format!("/game-passes/v1/universes/{universe_id}/game-passes");
    submit_multipart(ctx, "gamepass create", &path, "POST", fields, image).await
}

/// Update a Game Pass.
#[allow(clippy::too_many_arguments)]
pub async fn gamepass_update(
    ctx: &Ctx,
    universe_id: u64,
    game_pass_id: u64,
    name: Option<&str>,
    description: Option<&str>,
    price: Option<u64>,
    image: Option<&std::path::PathBuf>,
) -> Output {
    let mut fields: Vec<(&str, String)> = Vec::new();
    if let Some(n) = name {
        fields.push(("name", n.to_string()));
    }
    if let Some(d) = description {
        fields.push(("description", d.to_string()));
    }
    if let Some(p) = price {
        fields.push(("price", p.to_string()));
        fields.push(("isForSale", "true".to_string()));
    }
    if fields.is_empty() && image.is_none() {
        return failure(
            "gamepass update",
            ErrorBody {
                code: "nothing_to_update",
                message: "no fields to update".into(),
                status: None,
                detail: Some("Pass --name, --description, --price, or --image".into()),
            },
        );
    }
    let path = format!("/game-passes/v1/universes/{universe_id}/game-passes/{game_pass_id}");
    submit_multipart(ctx, "gamepass update", &path, "PATCH", fields, image).await
}

/// List Developer Products.
pub async fn devproduct_list(ctx: &Ctx, universe_id: u64) -> Output {
    let path = format!("/developer-products/v2/universes/{universe_id}/developer-products/creator");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("developerProducts")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "devproduct list",
                format!("Fetched {count} Developer Products"),
                Some(data),
            )
        }
        Err(err) => failure("devproduct list", err),
    }
}

/// Create a Developer Product.
pub async fn devproduct_create(
    ctx: &Ctx,
    universe_id: u64,
    name: &str,
    description: Option<&str>,
    price: Option<u64>,
    image: Option<&std::path::PathBuf>,
) -> Output {
    let mut fields: Vec<(&str, String)> = vec![("name", name.to_string())];
    if let Some(d) = description {
        fields.push(("description", d.to_string()));
    }
    if let Some(p) = price {
        fields.push(("price", p.to_string()));
        fields.push(("isForSale", "true".to_string()));
    }
    let path = format!("/developer-products/v2/universes/{universe_id}/developer-products");
    submit_multipart(ctx, "devproduct create", &path, "POST", fields, image).await
}

/// Build and send the multipart body. The image is optional.
async fn submit_multipart(
    ctx: &Ctx,
    command: &str,
    path: &str,
    method: &str,
    fields: Vec<(&str, String)>,
    image: Option<&std::path::PathBuf>,
) -> Output {
    // Update responses carry no body, so remember a caller-known id for the
    // message. Create responses do carry one, which takes precedence below.
    let known_id = known_id_from_path(path);
    let mut form = reqwest::multipart::Form::new();
    for (k, v) in fields {
        form = form.text(k.to_string(), v);
    }
    if let Some(img) = image {
        let bytes = match tokio::fs::read(img).await {
            Ok(b) => b,
            Err(e) => {
                return failure(
                    command,
                    ErrorBody {
                        code: "file_unreadable",
                        message: format!("cannot read the image: {}", img.display()),
                        status: None,
                        detail: Some(e.to_string()),
                    },
                );
            }
        };
        let name = img
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("icon.png")
            .to_string();
        let lower = name.to_ascii_lowercase();
        let ct = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            "image/jpeg"
        } else {
            "image/png"
        };
        let part = match reqwest::multipart::Part::bytes(bytes)
            .file_name(name)
            .mime_str(ct)
        {
            Ok(p) => p,
            Err(e) => {
                return failure(
                    command,
                    ErrorBody {
                        code: "mime_invalid",
                        message: format!("invalid content-type: {e}"),
                        status: None,
                        detail: None,
                    },
                );
            }
        };
        form = form.part("imageFile", part);
    }

    let url = ctx.url(path);
    let req = if method == "PATCH" {
        ctx.client.patch(url).multipart(form)
    } else {
        ctx.client.post(url).multipart(form)
    };

    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure(command, err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            // The id comes back as a number for monetization resources, but
            // some endpoints return a string. Accept either, then fall back to
            // the id in the URL when the body is empty (PATCH).
            let id = ["gamePassId", "productId", "developerProductId", "id"]
                .iter()
                .find_map(|k| data.get(*k))
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .or_else(|| known_id.clone())
                .unwrap_or_else(|| "(unknown)".to_string());
            Output::success(
                command,
                format!("{command} succeeded (id {id})"),
                Some(data),
            )
            .with_hints(vec!["This API is Beta; the schema may change".into()])
        }
        Err(err) => failure(command, err),
    }
}

/// Extract a trailing numeric id from a resource path, for update calls whose
/// response has no body. Returns None when the last segment is not a number.
fn known_id_from_path(path: &str) -> Option<String> {
    let last = path.trim_end_matches('/').rsplit('/').next()?;
    if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit()) {
        Some(last.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod path_id_tests {
    use super::known_id_from_path;

    #[test]
    fn known_id_from_path_takes_trailing_number() {
        assert_eq!(
            known_id_from_path("/game-passes/v1/universes/1/game-passes/2029442313"),
            Some("2029442313".to_string())
        );
        // A trailing slash is tolerated.
        assert_eq!(known_id_from_path("/a/b/42/"), Some("42".to_string()));
    }

    #[test]
    fn known_id_from_path_ignores_non_numeric_tails() {
        assert_eq!(
            known_id_from_path("/game-passes/v1/universes/1/game-passes"),
            None
        );
        assert_eq!(known_id_from_path("/a/b/abc"), None);
    }
}
