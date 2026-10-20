//! Monetization (Game Passes / Developer Products).
//!
//! Open Cloud took these over on 2025-12-04. Images ride in the same multipart
//! request. Still labeled Beta in the official docs.
//!
//! API keys scope permissions per universe.

use serde_json::Value;

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

#[derive(Clone, Copy)]
enum MonetizationKind {
    GamePass,
    DevProduct,
}

impl MonetizationKind {
    fn list_endpoint(self, universe_id: u64) -> (String, &'static str, &'static str, &'static str) {
        match self {
            Self::GamePass => (
                format!("/game-passes/v1/universes/{universe_id}/game-passes/creator"),
                "gamepass list",
                "gamePasses",
                "Game Passes",
            ),
            Self::DevProduct => (
                format!(
                    "/developer-products/v2/universes/{universe_id}/developer-products/creator"
                ),
                "devproduct list",
                "developerProducts",
                "Developer Products",
            ),
        }
    }

    fn create_endpoint(self, universe_id: u64) -> (String, &'static str) {
        match self {
            Self::GamePass => (
                format!("/game-passes/v1/universes/{universe_id}/game-passes"),
                "gamepass create",
            ),
            Self::DevProduct => (
                format!("/developer-products/v2/universes/{universe_id}/developer-products"),
                "devproduct create",
            ),
        }
    }
}

async fn list_monetization(
    ctx: &Ctx,
    kind: MonetizationKind,
    universe_id: u64,
    page_token: Option<&str>,
) -> Output {
    let (path, command, array_key, label) = kind.list_endpoint(universe_id);
    list_collection(ctx, command, &path, array_key, label, page_token).await
}

#[allow(clippy::too_many_arguments)]
async fn create_monetization(
    ctx: &Ctx,
    kind: MonetizationKind,
    universe_id: u64,
    name: &str,
    description: Option<&str>,
    price: Option<u64>,
    image: Option<&std::path::PathBuf>,
) -> Output {
    let (path, command) = kind.create_endpoint(universe_id);
    create_named_product(ctx, command, &path, name, description, price, image).await
}

/// List Game Passes.
pub async fn gamepass_list(ctx: &Ctx, universe_id: u64, page_token: Option<&str>) -> Output {
    list_monetization(ctx, MonetizationKind::GamePass, universe_id, page_token).await
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
    create_monetization(
        ctx,
        MonetizationKind::GamePass,
        universe_id,
        name,
        description,
        price,
        image,
    )
    .await
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
    is_for_sale: Option<bool>,
    image: Option<&std::path::PathBuf>,
) -> Output {
    let mut fields: Vec<(&'static str, String)> = Vec::new();
    append_optional_product_fields(&mut fields, name, description, price);
    if let Some(for_sale) = is_for_sale {
        fields.push(("isForSale", for_sale.to_string()));
    }
    if fields.is_empty() && image.is_none() {
        return failure(
            "gamepass update",
            ErrorBody {
                code: "nothing_to_update",
                message: "no fields to update".into(),
                status: None,
                detail: Some(
                    "Pass --name, --description, --price, --is-for-sale, or --image. --price does not change sale state."
                        .into(),
                ),

                refs: None,
            },
        );
    }
    let path = format!("/game-passes/v1/universes/{universe_id}/game-passes/{game_pass_id}");
    submit_multipart(ctx, "gamepass update", &path, "PATCH", fields, image).await
}

/// List Developer Products.
pub async fn devproduct_list(ctx: &Ctx, universe_id: u64, page_token: Option<&str>) -> Output {
    list_monetization(ctx, MonetizationKind::DevProduct, universe_id, page_token).await
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
    create_monetization(
        ctx,
        MonetizationKind::DevProduct,
        universe_id,
        name,
        description,
        price,
        image,
    )
    .await
}

async fn list_collection(
    ctx: &Ctx,
    command: &str,
    path: &str,
    array_key: &str,
    label: &str,
    page_token: Option<&str>,
) -> Output {
    let data = if let Some(token) = page_token.filter(|token| !token.is_empty()) {
        ctx.get_json_query(path, &[("pageToken", token)]).await
    } else {
        ctx.get_json(path).await
    };
    match data {
        Ok(data) => {
            let count = data
                .get(array_key)
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let mut out = Output::success(
                command,
                format!("Fetched {count} {label}"),
                Some(data.clone()),
            );
            if let Some(token) = data.get("nextPageToken").and_then(|v| v.as_str())
                && !token.is_empty()
            {
                out = out.with_hints([format!("Pass --page-token {token} to fetch the next page")]);
            }
            out
        }
        Err(err) => failure(command, err),
    }
}

fn append_description_and_price(
    fields: &mut Vec<(&'static str, String)>,
    description: Option<&str>,
    price: Option<u64>,
) {
    if let Some(d) = description {
        fields.push(("description", d.to_string()));
    }
    if let Some(p) = price {
        fields.push(("price", p.to_string()));
    }
}

fn create_product_fields(
    name: &str,
    description: Option<&str>,
    price: Option<u64>,
) -> Vec<(&'static str, String)> {
    let mut fields = vec![("name", name.to_string())];
    append_description_and_price(&mut fields, description, price);
    // Create is a new product. A supplied price also puts it on sale.
    // Updates do not inherit this coupling; sale state is an explicit field.
    if price.is_some() {
        fields.push(("isForSale", "true".to_string()));
    }
    fields
}

fn append_optional_product_fields(
    fields: &mut Vec<(&'static str, String)>,
    name: Option<&str>,
    description: Option<&str>,
    price: Option<u64>,
) {
    if let Some(n) = name {
        fields.push(("name", n.to_string()));
    }
    append_description_and_price(fields, description, price);
}

async fn create_named_product(
    ctx: &Ctx,
    command: &str,
    path: &str,
    name: &str,
    description: Option<&str>,
    price: Option<u64>,
    image: Option<&std::path::PathBuf>,
) -> Output {
    let fields = create_product_fields(name, description, price);
    submit_multipart(ctx, command, path, "POST", fields, image).await
}

/// Build and send the multipart body. The image is optional.
async fn submit_multipart(
    ctx: &Ctx,
    command: &str,
    path: &str,
    method: &str,
    fields: Vec<(&'static str, String)>,
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

                        refs: None,
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

                        refs: None,
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
            let data = if resp.status() == reqwest::StatusCode::NO_CONTENT && method == "PATCH" {
                Value::Null
            } else {
                match ctx.json_or_error(resp).await {
                    Ok(data) => data,
                    Err(err) => return failure(command, err),
                }
            };
            // The id comes back as a number for monetization resources, but
            // some endpoints return a string. Accept either, then fall back to
            // the id in the URL when the body is empty (PATCH).
            let id = ["gamePassId", "productId", "developerProductId", "id"]
                .iter()
                .filter_map(|k| data.get(*k))
                .find_map(|value| {
                    value
                        .as_u64()
                        .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                        .filter(|id| *id > 0)
                        .map(|id| id.to_string())
                })
                .or_else(|| known_id.clone());
            let Some(id) = id else {
                return failure(command, ErrorBody {
                    code: "resource_id_missing",
                    message: "Creation response contains no valid resource ID; outcome may be uncertain".into(),
                    status: None,
                    detail: Some("Inspect existing passes/products on Creator Dashboard before repeating creation".into()),
                    refs: None,
                });
            };
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
