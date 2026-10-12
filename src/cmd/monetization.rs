//! 課金商品（Game Pass / Developer Product）。
//!
//! 2025-12-04 以降 Open Cloud 化された。multipart で画像を同一リクエストに
//! 載せられる。公式ドキュメント上は現在も Beta。
//!
//! API key は universe 単位で権限を切る。

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// Game Pass 一覧。
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
                format!("Game Pass {count} 件を取得しました"),
                Some(data),
            )
        }
        Err(err) => failure("gamepass list", err),
    }
}

/// Game Pass 作成。
#[allow(clippy::too_many_arguments)]
pub async fn gamepass_create(
    ctx: &Ctx,
    universe_id: u64,
    name: &str,
    description: Option<&str>,
    price: Option<u64>,
    image: Option<&std::path::PathBuf>,
) -> Output {
    let mut request = json!({ "name": name });
    if let Some(d) = description {
        request["description"] = json!(d);
    }
    if let Some(p) = price {
        request["price"] = json!(p);
        request["isForSale"] = json!(true);
    }
    let path = format!("/game-passes/v1/universes/{universe_id}/game-passes");
    submit_with_optional_image(ctx, "gamepass create", &path, "POST", request, image).await
}

/// Game Pass 更新。
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
    let mut request = json!({ "gamePassId": game_pass_id.to_string() });
    if let Some(n) = name {
        request["name"] = json!(n);
    }
    if let Some(d) = description {
        request["description"] = json!(d);
    }
    if let Some(p) = price {
        request["price"] = json!(p);
        request["isForSale"] = json!(true);
    }
    let path = format!("/game-passes/v1/universes/{universe_id}/game-passes/{game_pass_id}");
    submit_with_optional_image(ctx, "gamepass update", &path, "PATCH", request, image).await
}

/// Developer Product 一覧。
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
                format!("Developer Product {count} 件を取得しました"),
                Some(data),
            )
        }
        Err(err) => failure("devproduct list", err),
    }
}

/// Developer Product 作成。
pub async fn devproduct_create(
    ctx: &Ctx,
    universe_id: u64,
    name: &str,
    description: Option<&str>,
    price: Option<u64>,
    image: Option<&std::path::PathBuf>,
) -> Output {
    let mut request = json!({ "name": name });
    if let Some(d) = description {
        request["description"] = json!(d);
    }
    if let Some(p) = price {
        request["price"] = json!(p);
        request["isForSale"] = json!(true);
    }
    let path = format!("/developer-products/v2/universes/{universe_id}/developer-products");
    submit_with_optional_image(ctx, "devproduct create", &path, "POST", request, image).await
}

/// multipart を組み立てて送る。画像は任意。
async fn submit_with_optional_image(
    ctx: &Ctx,
    command: &str,
    path: &str,
    method: &str,
    request: Value,
    image: Option<&std::path::PathBuf>,
) -> Output {
    let mut form = reqwest::multipart::Form::new().text("request", request.to_string());
    if let Some(img) = image {
        let bytes = match tokio::fs::read(img).await {
            Ok(b) => b,
            Err(e) => {
                return failure(
                    command,
                    ErrorBody {
                        code: "file_unreadable",
                        message: format!("画像を読めません: {}", img.display()),
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
                        message: format!("content-type が不正です: {e}"),
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
            let id = data
                .get("gamePassId")
                .or_else(|| data.get("developerProductId"))
                .or_else(|| data.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)");
            Output::success(
                command,
                format!("{command} が成功しました（id {id}）"),
                Some(data),
            )
            .with_hints(vec![
                "この API は Beta です。スキーマが変わる可能性があります".into(),
            ])
        }
        Err(err) => failure(command, err),
    }
}
