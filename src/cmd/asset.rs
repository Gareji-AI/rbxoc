//! Asset operations.
//!
//! create / update send `multipart/form-data` carrying `request` (JSON) and
//! `fileContent` (binary) in one request. The response is an Operation ID for
//! an asynchronous job, so poll it with `asset operation`.
//!
//! Supported formats (20 MB per request):
//! - Model: .fbx .gltf .glb .rbxm .rbxmx
//! - Image / Decal: .png .jpeg .bmp .tga
//! - Audio: .mp3 .ogg .wav .flac
//! - Video: .mp4 .mov
//!
//! Only Models can replace the content body on update. Icons and previews must
//! be Image assets.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

const CMD_CREATE: &str = "asset create";
const CMD_UPDATE: &str = "asset update";

/// Decide assetType and content-type from the extension. Returns `None` when
/// the guess is unsafe, so the caller can ask for an explicit type.
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

/// Asset types whose content body can be replaced on update.
fn is_updateable_body(asset_type: &str) -> bool {
    asset_type == "Model"
}

/// Upload a new asset.
#[allow(clippy::too_many_arguments)]
pub async fn create(
    ctx: &Ctx,
    file: &std::path::Path,
    display_name: &str,
    description: &str,
    creator_user_id: Option<u64>,
    creator_group_id: Option<u64>,
    asset_type_override: Option<&str>,
) -> Output {
    let (asset_type, content_type) = match resolve_asset_type(file, asset_type_override, CMD_CREATE)
    {
        Ok(v) => v,
        Err(err) => return failure(CMD_CREATE, err),
    };

    let creator = match (creator_user_id, creator_group_id) {
        (Some(id), None) => json!({ "userId": id.to_string() }),
        (None, Some(id)) => json!({ "groupId": id.to_string() }),
        (Some(_), Some(_)) => {
            return failure(
                CMD_CREATE,
                ErrorBody {
                    code: "creator_ambiguous",
                    message: "userId and groupId cannot be set together".into(),
                    status: None,
                    detail: None,
                },
            );
        }
        (None, None) => {
            return failure(
                CMD_CREATE,
                ErrorBody {
                    code: "creator_missing",
                    message: "either userId or groupId is required".into(),
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

    let form = match build_multipart(&request, file, content_type, CMD_CREATE).await {
        Ok(f) => f,
        Err(err) => return failure(CMD_CREATE, err),
    };

    let req = ctx
        .client
        .post(ctx.url("/assets/v1/assets"))
        .multipart(form);
    submit_operation(ctx, req, CMD_CREATE, &asset_type).await
}

/// Update an asset.
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
    let (resolved_type, content_type) = match resolve_asset_type(file, asset_type, CMD_UPDATE) {
        Ok(v) => v,
        Err(err) => return failure(CMD_UPDATE, err),
    };

    if !is_updateable_body(&resolved_type) {
        return failure(
            CMD_UPDATE,
            ErrorBody {
                code: "asset_update_unsupported",
                message: format!("{resolved_type} cannot replace its content body; only Model can"),
                status: None,
                detail: Some(
                    "To change metadata only, pass --display-name / --description and omit --file"
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

    let mut form = match build_multipart(&request, file, content_type, CMD_UPDATE).await {
        Ok(f) => f,
        Err(err) => return failure(CMD_UPDATE, err),
    };

    // Icons and previews ride in the same request as Image assets.
    if let Some(icon) = icon_file
        && let Err(err) = add_image_part(&mut form, icon, "icon", CMD_UPDATE).await
    {
        return failure(CMD_UPDATE, err);
    }
    if let Some(preview) = preview_file
        && let Err(err) = add_image_part(&mut form, preview, "preview", CMD_UPDATE).await
    {
        return failure(CMD_UPDATE, err);
    }

    let update_mask = build_update_mask(
        display_name.is_some(),
        description.is_some(),
        icon_file.is_some(),
        preview_file.is_some(),
    );
    let url = format!("/assets/v1/assets/{asset_id}?updateMask={update_mask}");
    let req = ctx.client.patch(ctx.url(&url)).multipart(form);
    submit_operation(ctx, req, CMD_UPDATE, &resolved_type).await
}

/// Resolve assetType and content-type, or explain why it could not.
fn resolve_asset_type(
    file: &std::path::Path,
    asset_type_override: Option<&str>,
    command: &str,
) -> Result<(String, &'static str), ErrorBody> {
    match asset_type_override {
        Some(t) => match content_type_for(t, file) {
            Some(ct) => Ok((t.to_string(), ct)),
            None => Err(ErrorBody {
                code: "asset_type_unknown",
                message: format!("cannot map --asset-type {t} to a content-type"),
                status: None,
                detail: Some(format!(
                    "the extension of {} is not known for {t}",
                    file.display()
                )),
            }),
        },
        None => match infer_asset(file) {
            Some((t, ct)) => Ok((t.to_string(), ct)),
            None => Err(ErrorBody {
                code: "file_type_unknown",
                message: format!("cannot infer assetType from {}", file.display()),
                status: None,
                detail: Some(format!(
                    "{command} needs --asset-type when the extension is not recognized"
                )),
            }),
        },
    }
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
    command: &str,
) -> Result<reqwest::multipart::Form, ErrorBody> {
    let bytes = tokio::fs::read(file).await.map_err(|e| ErrorBody {
        code: "file_unreadable",
        message: format!("cannot read the file: {}", file.display()),
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
            message: format!("invalid content-type: {e}"),
            status: None,
            detail: Some(format!("{command} could not build the multipart body")),
        })?;
    Ok(reqwest::multipart::Form::new()
        .text("request", request.to_string())
        .part("fileContent", part))
}

async fn add_image_part(
    form: &mut reqwest::multipart::Form,
    file: &std::path::Path,
    field: &str,
    command: &str,
) -> Result<(), ErrorBody> {
    let bytes = tokio::fs::read(file).await.map_err(|e| ErrorBody {
        code: "file_unreadable",
        message: format!("cannot read the image: {}", file.display()),
        status: None,
        detail: Some(e.to_string()),
    })?;
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.png")
        .to_string();
    let lower = name.to_ascii_lowercase();
    let ct = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        "image/png"
    };
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(name)
        .mime_str(ct)
        .map_err(|e| ErrorBody {
            code: "mime_invalid",
            message: format!("invalid content-type: {e}"),
            status: None,
            detail: Some(format!("{command} could not build the image part")),
        })?;
    // Form has no push API; take it and rebuild.
    let taken = std::mem::replace(form, reqwest::multipart::Form::new());
    *form = taken.part(field.to_string(), part);
    Ok(())
}

/// Shared handling for endpoints that return an Operation.
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
                        format!("Accepted the {asset_type} operation (operation {op_id})"),
                        Some(json!({ "operationId": op_id, "response": data })),
                    )
                    .with_hints(vec![
                        format!(
                            "Run `rbxoc asset operation --operation-id {op_id}` to check completion"
                        ),
                        "Processing is asynchronous; it is not immediate".into(),
                    ])
                }
                Err(e) => failure(
                    command,
                    ErrorBody {
                        code: "invalid_json",
                        message: format!("cannot parse the response: {e}"),
                        status: None,
                        detail: None,
                    },
                ),
            }
        }
        Err(err) => failure(command, err),
    }
}

/// Fetch asset metadata.
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
                .unwrap_or("(unnamed)");
            Output::success(
                "asset get",
                format!("Fetched asset {asset_id}: {name}"),
                Some(data),
            )
        }
        Err(err) => failure("asset get", err),
    }
}

/// List an asset's versions.
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
                format!("Fetched {count} versions for asset {asset_id}"),
                Some(data),
            )
        }
        Err(err) => failure("asset versions", err),
    }
}

/// Roll an asset back to a given version.
pub async fn rollback(ctx: &Ctx, asset_id: u64, version: u64) -> Output {
    let path = format!("/assets/v1/assets/{asset_id}/versions:rollback");
    let body = json!({ "assetVersion": version.to_string() });
    let req = ctx.client.post(ctx.url(&path)).json(&body);
    submit_operation(ctx, req, "asset rollback", "asset").await
}

/// Archive an asset.
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
                format!("Archived asset {asset_id}"),
                Some(json!({ "assetId": asset_id })),
            )
        }
        Err(err) => failure("asset archive", err),
    }
}

/// Check an operation's status.
pub async fn operation(ctx: &Ctx, operation_id: &str) -> Output {
    let path = format!("/assets/v1/operations/{operation_id}");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let done = data.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
            let state = if done { "completed" } else { "still running" };
            let mut out = Output::success(
                "asset operation",
                format!("Operation {operation_id} is {state}"),
                Some(data),
            );
            if !done {
                out = out.with_hints(vec![
                    "Wait a few seconds and run the same command again".into(),
                ]);
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
