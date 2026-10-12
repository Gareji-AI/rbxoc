//! アセット操作。
//!
//! create / update は `multipart/form-data` で `request`（JSON）と
//! `fileContent`（バイナリ）を同一リクエストに載せる。応答は Operation ID を
//! 返す非同期処理なので、`asset operation` で poll する。
//!
//! 対応形式（20MB / リクエスト）:
//! - Model: .fbx .gltf .glb .rbxm .rbxmx
//! - Image / Decal: .png .jpeg .bmp .tga
//! - Audio: .mp3 .ogg .wav .flac
//! - Video: .mp4 .mov
//!
//! Update で content body を差し替えられるのは Model のみ。icon と preview は
//! Image アセットである必要がある。

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// 拡張子から assetType と content-type を決める。
/// 推測できない場合は `None` を返し、呼び出し側が明示指定を促す。
pub fn infer_asset(file: &std::path::Path) -> Option<(&'static str, &'static str)> {
    let ext = file.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "fbx" => ("Model", "model/fbx"),
        "gltf" => ("Model", "model/gltf+json"),
        "glb" => ("Model", "model/gltf-binary"),
        "rbxm" => ("Model", "model/x-rbxm"),
        "rbxmx" => ("Model", "model/x-rbxm"),
        "png" => ("Decal", "image/png"),
        "jpg" | "jpeg" => ("Decal", "image/jpeg"),
        "bmp" => ("Decal", "image/bmp"),
        "tga" => ("Decal", "image/tga"),
        "mp3" => ("Audio", "audio/mpeg"),
        "ogg" => ("Audio", "audio/ogg"),
        "wav" => ("Audio", "audio/wav"),
        "flac" => ("Audio", "audio/flac"),
        "mp4" => ("Video", "video/mp4"),
        "mov" => ("Video", "video/mov"),
        _ => return None,
    })
}

/// 更新できない種別。create は可、update の content body 差し替えは不可。
fn is_updateable_body(asset_type: &str) -> bool {
    asset_type == "Model"
}

/// アセットを作成する。
pub async fn create(
    ctx: &Ctx,
    file: &std::path::Path,
    display_name: &str,
    description: &str,
    creator_user_id: Option<u64>,
    creator_group_id: Option<u64>,
    asset_type_override: Option<&str>,
) -> Output {
    let (asset_type, content_type) = match asset_type_override {
        Some(t) => match content_type_for(t, file) {
            Some(ct) => (t.to_string(), ct),
            None => {
                return failure(
                    "asset create",
                    ErrorBody {
                        code: "asset_type_unknown",
                        message: format!("--asset-type {t} の content-type を決められません"),
                        status: None,
                        detail: None,
                    },
                );
            }
        },
        None => match infer_asset(file) {
            Some((t, ct)) => (t.to_string(), ct),
            None => {
                return failure(
                    "asset create",
                    ErrorBody {
                        code: "file_type_unknown",
                        message: format!(
                            "拡張子から assetType を推測できません: {}",
                            file.display()
                        ),
                        status: None,
                        detail: None,
                    },
                );
            }
        },
    };

    let creator = match (creator_user_id, creator_group_id) {
        (Some(id), None) => json!({ "userId": id.to_string() }),
        (None, Some(id)) => json!({ "groupId": id.to_string() }),
        (Some(_), Some(_)) => {
            return failure(
                "asset create",
                ErrorBody {
                    code: "creator_ambiguous",
                    message: "userId と groupId は同時に指定できません".into(),
                    status: None,
                    detail: None,
                },
            );
        }
        (None, None) => {
            return failure(
                "asset create",
                ErrorBody {
                    code: "creator_missing",
                    message: "userId か groupId が必要です".into(),
                    status: None,
                    detail: None,
                },
            );
        }
    };

    let request = json!({
        "assetType": asset_type,
        "displayName": display_name,
        "description": description,
        "creationContext": { "creator": creator },
    });

    let form = build_multipart(&request, file, content_type).await;
    let form = match form {
        Ok(f) => f,
        Err(err) => return failure("asset create", err),
    };

    let req = ctx
        .client
        .post(ctx.url("/assets/v1/assets"))
        .multipart(form);
    submit_operation(ctx, req, "asset create", &asset_type).await
}

/// アセットを更新する。
#[allow(clippy::too_many_arguments)]
pub async fn update(
    ctx: &Ctx,
    asset_id: u64,
    file: &std::path::Path,
    asset_type: Option<&str>,
    display_name: Option<&str>,
    description: Option<&str>,
    icon_file: Option<&std::path::Path>,
    preview_file: Option<&std::path::Path>,
) -> Output {
    let (resolved_type, content_type) = match asset_type {
        Some(t) => match content_type_for(t, file) {
            Some(ct) => (t.to_string(), ct),
            None => {
                return failure(
                    "asset update",
                    ErrorBody {
                        code: "asset_type_unknown",
                        message: format!("--asset-type {t} の content-type を決められません"),
                        status: None,
                        detail: None,
                    },
                );
            }
        },
        None => match infer_asset(file) {
            Some((t, ct)) => (t.to_string(), ct),
            None => {
                return failure(
                    "asset update",
                    ErrorBody {
                        code: "file_type_unknown",
                        message: format!(
                            "拡張子から assetType を推測できません: {}",
                            file.display()
                        ),
                        status: None,
                        detail: None,
                    },
                );
            }
        },
    };

    if !is_updateable_body(&resolved_type) {
        return failure(
            "asset update",
            ErrorBody {
                code: "asset_update_unsupported",
                message: format!(
                    "{resolved_type} は content body を更新できません。更新できるのは Model のみです"
                ),
                status: None,
                detail: Some(
                    "メタデータのみ更新する場合は --display-name / --description を使い、--file を省略してください"
                        .into(),
                ),
            },
        );
    }

    let mut request = json!({ "assetType": resolved_type, "assetId": asset_id.to_string() });
    if let Some(n) = display_name {
        request["displayName"] = json!(n);
    }
    if let Some(d) = description {
        request["description"] = json!(d);
    }

    let form = build_multipart(&request, file, content_type).await;
    let mut form = match form {
        Ok(f) => f,
        Err(err) => return failure("asset update", err),
    };

    // icon と preview は Image アセットとして同一リクエストへ載せる。
    if let Some(icon) = icon_file {
        match add_image_part(&mut form, icon, "icon").await {
            Ok(()) => {}
            Err(err) => return failure("asset update", err),
        }
    }
    if let Some(preview) = preview_file {
        match add_image_part(&mut form, preview, "preview").await {
            Ok(()) => {}
            Err(err) => return failure("asset update", err),
        }
    }

    let update_mask = build_update_mask(
        display_name.is_some(),
        description.is_some(),
        icon_file.is_some(),
        preview_file.is_some(),
    );
    let url = format!("/assets/v1/assets/{asset_id}?updateMask={update_mask}");
    let req = ctx.client.patch(ctx.url(&url)).multipart(form);
    submit_operation(ctx, req, "asset update", &resolved_type).await
}

fn build_update_mask(name: bool, desc: bool, icon: bool, preview: bool) -> String {
    let mut parts: Vec<&str> = vec!["assetType", "assetId"];
    if name {
        parts.push("displayName");
    }
    if desc {
        parts.push("description");
    }
    if icon {
        parts.push("icon");
    }
    if preview {
        parts.push("preview");
    }
    parts.join(",")
}

fn content_type_for(asset_type: &str, file: &std::path::Path) -> Option<&'static str> {
    let ext = file.extension()?.to_str()?.to_ascii_lowercase();
    Some(match (asset_type, ext.as_str()) {
        ("Model", "fbx") => "model/fbx",
        ("Model", "gltf") => "model/gltf+json",
        ("Model", "glb") => "model/gltf-binary",
        ("Model", "rbxm") | ("Model", "rbxmx") => "model/x-rbxm",
        ("Decal", "png") => "image/png",
        ("Decal", "jpg") | ("Decal", "jpeg") => "image/jpeg",
        ("Decal", "bmp") => "image/bmp",
        ("Decal", "tga") => "image/tga",
        ("Audio", "mp3") => "audio/mpeg",
        ("Audio", "ogg") => "audio/ogg",
        ("Audio", "wav") => "audio/wav",
        ("Audio", "flac") => "audio/flac",
        ("Video", "mp4") => "video/mp4",
        ("Video", "mov") => "video/mov",
        _ => return None,
    })
}

async fn build_multipart(
    request: &Value,
    file: &std::path::Path,
    content_type: &'static str,
) -> Result<reqwest::multipart::Form, ErrorBody> {
    let bytes = tokio::fs::read(file).await.map_err(|e| ErrorBody {
        code: "file_unreadable",
        message: format!("ファイルを読めません: {}", file.display()),
        status: None,
        detail: Some(e.to_string()),
    })?;
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("asset")
        .to_string();
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str(content_type)
        .map_err(|e| ErrorBody {
            code: "mime_invalid",
            message: format!("content-type が不正です: {e}"),
            status: None,
            detail: None,
        })?;
    Ok(reqwest::multipart::Form::new()
        .text("request", request.to_string())
        .part("fileContent", part))
}

async fn add_image_part(
    form: &mut reqwest::multipart::Form,
    file: &std::path::Path,
    field: &str,
) -> Result<(), ErrorBody> {
    let bytes = tokio::fs::read(file).await.map_err(|e| ErrorBody {
        code: "file_unreadable",
        message: format!("画像を読めません: {}", file.display()),
        status: None,
        detail: Some(e.to_string()),
    })?;
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.png")
        .to_string();
    let ct = if name.to_ascii_lowercase().ends_with(".jpg")
        || name.to_ascii_lowercase().ends_with(".jpeg")
    {
        "image/jpeg"
    } else {
        "image/png"
    };
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(name)
        .mime_str(ct)
        .map_err(|e| ErrorBody {
            code: "mime_invalid",
            message: format!("content-type が不正です: {e}"),
            status: None,
            detail: None,
        })?;
    // 既存の Form は take して再構築する。
    let taken = std::mem::replace(form, reqwest::multipart::Form::new());
    *form = taken.part(field.to_string(), part);
    Ok(())
}

/// operation を返すエンドポイントの共通処理。
async fn submit_operation(
    ctx: &Ctx,
    req: reqwest::RequestBuilder,
    command: &str,
    asset_type: &str,
) -> Output {
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure(command, err);
            }
            match resp.json::<Value>().await {
                Ok(data) => {
                    let op = data
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(unknown)");
                    let op_id = op.strip_prefix("operations/").unwrap_or(op);
                    Output::success(
                        command,
                        format!("{asset_type} の処理を受け付けました（operation {op_id}）"),
                        Some(json!({ "operationId": op_id, "response": data })),
                    )
                    .with_hints(vec![
                        format!(
                            "`rbxoc asset operation --operation-id {op_id}` で完了を確認できます"
                        ),
                        "処理は非同期です。すぐには反映されません".into(),
                    ])
                }
                Err(e) => failure(
                    command,
                    ErrorBody {
                        code: "invalid_json",
                        message: format!("応答を解釈できません: {e}"),
                        status: None,
                        detail: None,
                    },
                ),
            }
        }
        Err(err) => failure(command, err),
    }
}

/// アセットのメタデータを取得する。
pub async fn get(ctx: &Ctx, asset_id: u64, read_mask: Option<&str>) -> Output {
    let mut path = format!("/assets/v1/assets/{asset_id}");
    if let Some(mask) = read_mask {
        path.push_str(&format!("?readMask={mask}"));
    }
    match ctx.get_json(&path).await {
        Ok(data) => {
            let name = data
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("(名称なし)");
            Output::success(
                "asset get",
                format!("アセット {asset_id} を取得しました: {name}"),
                Some(data),
            )
        }
        Err(err) => failure("asset get", err),
    }
}

/// アセットのバージョン一覧。
pub async fn versions(ctx: &Ctx, asset_id: u64, max_page_size: u32) -> Output {
    let path = format!("/assets/v1/assets/{asset_id}/versions?maxPageSize={max_page_size}");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("assetVersions")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "asset versions",
                format!("アセット {asset_id} のバージョン {count} 件を取得しました"),
                Some(data),
            )
        }
        Err(err) => failure("asset versions", err),
    }
}

/// 指定バージョンへ巻き戻す。
pub async fn rollback(ctx: &Ctx, asset_id: u64, version: u64) -> Output {
    let path = format!("/assets/v1/assets/{asset_id}/versions:rollback");
    let body = json!({ "assetVersion": version.to_string() });
    let req = ctx.client.post(ctx.url(&path)).json(&body);
    submit_operation(ctx, req, "asset rollback", "asset").await
}

/// アセットをアーカイブする。
pub async fn archive(ctx: &Ctx, asset_id: u64) -> Output {
    let path = format!("/assets/v1/assets/{asset_id}:archive");
    let req = ctx.client.post(ctx.url(&path)).json(&json!({}));
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("asset archive", err);
            }
            Output::success(
                "asset archive",
                format!("アセット {asset_id} をアーカイブしました"),
                Some(json!({ "assetId": asset_id })),
            )
        }
        Err(err) => failure("asset archive", err),
    }
}

/// operation の状態を確認する。
pub async fn operation(ctx: &Ctx, operation_id: &str) -> Output {
    let path = format!("/assets/v1/operations/{operation_id}");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let done = data.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
            let summary = if done {
                "処理が完了しました".to_string()
            } else {
                "処理はまだ実行中です".to_string()
            };
            let mut out = Output::success(
                "asset operation",
                format!("{summary}（operation {operation_id}）"),
                Some(data),
            );
            if !done {
                out = out.with_hints(vec!["数秒待って同じコマンドを再実行してください".into()]);
            }
            out
        }
        Err(err) => failure("asset operation", err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn infers_model_formats() {
        assert_eq!(
            infer_asset(Path::new("a.fbx")),
            Some(("Model", "model/fbx"))
        );
        assert_eq!(
            infer_asset(Path::new("a.glb")),
            Some(("Model", "model/gltf-binary"))
        );
        assert_eq!(
            infer_asset(Path::new("a.rbxm")),
            Some(("Model", "model/x-rbxm"))
        );
    }

    #[test]
    fn infers_image_formats() {
        assert_eq!(
            infer_asset(Path::new("a.png")),
            Some(("Decal", "image/png"))
        );
        assert_eq!(
            infer_asset(Path::new("a.JPEG")),
            Some(("Decal", "image/jpeg"))
        );
    }

    #[test]
    fn unknown_extension_returns_none() {
        assert_eq!(infer_asset(Path::new("a.txt")), None);
        assert_eq!(infer_asset(Path::new("noext")), None);
    }

    #[test]
    fn only_model_body_is_updateable() {
        assert!(is_updateable_body("Model"));
        assert!(!is_updateable_body("Decal"));
        assert!(!is_updateable_body("Audio"));
    }

    #[test]
    fn update_mask_is_minimal() {
        assert_eq!(
            build_update_mask(false, false, false, false),
            "assetType,assetId"
        );
        assert_eq!(
            build_update_mask(true, true, true, true),
            "assetType,assetId,displayName,description,icon,preview"
        );
    }
}
