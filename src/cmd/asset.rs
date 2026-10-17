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
///
/// `.rbxm` / `.rbxmx` need the file body to distinguish Animation from Model,
/// so `infer_asset_from_file` handles those. This version assumes Model.
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

/// Decide assetType and content-type, inspecting the body for Roblox models.
///
/// Returns the resolved pair plus a note when the body changed the answer, so
/// the caller can surface why an `.rbxm` landed as an Animation.
pub async fn infer_asset_from_file(
    file: &std::path::Path,
) -> Result<(&'static str, &'static str, Option<String>), ErrorBody> {
    use crate::cmd::model::{self, ModelKind};

    let Some(format) = model::format_for(file) else {
        // Not a Roblox model; the extension decides.
        return infer_asset(file)
            .map(|(t, ct)| (t, ct, None))
            .ok_or_else(|| ErrorBody {
                code: "file_type_unknown",
                message: format!("cannot infer assetType from {}", file.display()),
                status: None,
                detail: None,

                refs: None,
            });
    };

    let bytes = tokio::fs::read(file).await.map_err(|e| ErrorBody {
        code: "file_unreadable",
        message: format!("cannot read the file: {}", file.display()),
        status: None,
        detail: Some(e.to_string()),

        refs: None,
    })?;

    match model::classify(&bytes, format) {
        Ok(root) => {
            let kind = root.kind();
            let mut note = if kind == ModelKind::Animation {
                Some(format!(
                    "{} has class {}, so it uploads as Animation",
                    file.display(),
                    root.class
                ))
            } else {
                None
            };
            if let Some(warning) =
                model::large_xml_animation_warning(file, kind.asset_type(), bytes.len() as u64)
                && let Some(note) = &mut note
            {
                note.push_str(". ");
                note.push_str(&warning);
            }
            Ok((kind.asset_type(), "model/x-rbxm", note))
        }
        Err(e) => Err(ErrorBody {
            code: "model_parse_failed",
            message: format!("cannot read {} as a Roblox {format} model", file.display()),
            status: None,
            detail: Some(e.to_string()),

            refs: None,
        }),
    }
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
    let (asset_type, content_type, type_note) =
        match resolve_asset_type(file, asset_type_override, CMD_CREATE).await {
            Ok((t, ct, note)) => (t, ct, note),
            Err(err) => return failure(CMD_CREATE, err),
        };

    let creator = match creator_json(creator_user_id, creator_group_id) {
        Ok(creator) => creator,
        Err(err) => return failure(CMD_CREATE, err),
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
    submit_operation(ctx, req, CMD_CREATE, &asset_type, type_note).await
}

/// Update an asset.
#[allow(clippy::too_many_arguments)]
pub async fn update(
    ctx: &Ctx,
    asset_id: u64,
    file: Option<&std::path::Path>,
    asset_type: Option<&str>,
    display_name: Option<&str>,
    description: Option<&str>,
    icon: Option<&str>,
    previews: &[String],
) -> Output {
    let plan = match plan_update(
        asset_id,
        file,
        asset_type,
        display_name,
        description,
        icon,
        previews,
    )
    .await
    {
        Ok(plan) => plan,
        Err(err) => return failure(CMD_UPDATE, err),
    };
    let form = match (file, plan.content_type) {
        (Some(file), Some(content_type)) => {
            match build_multipart(&plan.request, file, content_type, CMD_UPDATE).await {
                Ok(form) => form,
                Err(err) => return failure(CMD_UPDATE, err),
            }
        }
        _ => reqwest::multipart::Form::new().text("request", plan.request.to_string()),
    };
    let mut req = ctx
        .client
        .patch(ctx.url(&format!("/assets/v1/assets/{asset_id}")));
    if !plan.mask.is_empty() {
        req = req.query(&[("updateMask", &plan.mask)]);
    }
    let req = req.multipart(form);
    if file.is_some() {
        return submit_operation(ctx, req, CMD_UPDATE, &plan.asset_type, plan.type_note).await;
    }
    let data = match ctx.send(req).await {
        Ok(resp) => match ctx.json_or_error(resp).await {
            Ok(data) => data,
            Err(err) => return failure(CMD_UPDATE, err),
        },
        Err(err) => return failure(CMD_UPDATE, err),
    };
    if data
        .get("path")
        .and_then(Value::as_str)
        .is_some_and(|path| path.starts_with("operations/"))
    {
        return operation_output(data, CMD_UPDATE, "asset", None);
    }
    // Metadata-only examples return the updated fields directly. Do not
    // invent an operation receipt or interpret an empty object as success.
    let valid_field = |field: &str, value: &Value| match field {
        "previews" => value.is_array(),
        _ => value.is_string(),
    };
    let has_metadata = plan.mask.split(',').any(|field| {
        data.get(field)
            .is_some_and(|value| valid_field(field, value))
    });
    let malformed_metadata = plan.mask.split(',').any(|field| {
        data.get(field)
            .is_some_and(|value| !valid_field(field, value))
    });
    let target = format!("assets/{asset_id}");
    let has_path = data.get("path").and_then(Value::as_str) == Some(target.as_str());
    let wrong_path = data.get("path").is_some() && !has_path;
    let wrong_id = data.get("assetId").is_some_and(|id| {
        id.as_u64()
            .or_else(|| id.as_str().and_then(|s| s.parse().ok()))
            != Some(asset_id)
    });
    let has_error = data.get("error").is_some_and(|error| !error.is_null());
    if (!has_metadata && !has_path) || wrong_path || wrong_id || has_error || malformed_metadata {
        return failure(CMD_UPDATE, ErrorBody {
            code: "invalid_asset_response",
            message: "Metadata update response has no updated fields or matching asset resource".into(),
            status: None,
            detail: Some("Inspect the target asset before repeating the update; it was sent at most once".into()),
            refs: None,
        });
    }
    Output::success(
        CMD_UPDATE,
        format!("Updated metadata for asset {asset_id}"),
        Some(data),
    )
}

struct UpdatePlan {
    request: Value,
    mask: String,
    asset_type: String,
    content_type: Option<&'static str>,
    type_note: Option<String>,
}

fn image_resource(resource: &str) -> bool {
    resource.strip_prefix("assets/").is_some_and(|id| {
        !id.is_empty()
            && id.bytes().all(|b| b.is_ascii_digit())
            && id.parse::<u64>().is_ok_and(|id| id > 0)
    })
}

#[allow(clippy::too_many_arguments)]
async fn plan_update(
    asset_id: u64,
    file: Option<&std::path::Path>,
    asset_type: Option<&str>,
    display_name: Option<&str>,
    description: Option<&str>,
    icon: Option<&str>,
    previews: &[String],
) -> Result<UpdatePlan, ErrorBody> {
    let invalid = |code, message: &str| ErrorBody {
        code,
        message: message.into(),
        status: None,
        detail: None,
        refs: None,
    };
    if asset_id == 0 {
        return Err(invalid("invalid_asset_id", "--asset-id must be positive"));
    }
    if icon.is_some_and(|value| !image_resource(value))
        || previews.iter().any(|value| !image_resource(value))
    {
        return Err(invalid(
            "invalid_image_resource",
            "--icon and --preview require existing Image resources like assets/123, not local files or Decal IDs",
        ));
    }
    if file.is_none() && asset_type.is_some() {
        return Err(invalid(
            "asset_type_requires_file",
            "--asset-type applies only to content updates with --file",
        ));
    }
    let (asset_type, content_type, type_note) = if let Some(file) = file {
        let (kind, content_type, note) = resolve_asset_type(file, asset_type, CMD_UPDATE).await?;
        if !is_updateable_body(&kind) {
            return Err(invalid(
                "asset_update_unsupported",
                "Only Model content can be replaced; omit --file for a metadata-only update",
            ));
        }
        (kind, Some(content_type), note)
    } else {
        ("asset".into(), None, None)
    };
    let mask = build_update_mask(
        display_name.is_some(),
        description.is_some(),
        icon.is_some(),
        !previews.is_empty(),
    );
    if file.is_none() && mask.is_empty() {
        return Err(invalid(
            "nothing_to_update",
            "Provide --file or at least one metadata field",
        ));
    }
    let mut request = json!({"assetId": asset_id.to_string()});
    if let Some(name) = display_name {
        request["displayName"] = json!(name);
    }
    if let Some(description) = description {
        request["description"] = json!(description);
    }
    if let Some(icon) = icon {
        request["icon"] = json!(icon);
    }
    if !previews.is_empty() {
        request["previews"] = previews
            .iter()
            .map(|asset| json!({"asset": asset}))
            .collect();
    }
    Ok(UpdatePlan {
        request,
        mask,
        asset_type,
        content_type,
        type_note,
    })
}

/// Creator target for uploads. Exactly one of userId or groupId is required.
fn creator_json(user_id: Option<u64>, group_id: Option<u64>) -> Result<Value, ErrorBody> {
    match (user_id, group_id) {
        (Some(id), None) => Ok(json!({ "userId": id.to_string() })),
        (None, Some(id)) => Ok(json!({ "groupId": id.to_string() })),
        (Some(_), Some(_)) => Err(ErrorBody {
            code: "creator_ambiguous",
            message: "userId and groupId cannot be set together".into(),
            status: None,
            detail: None,

            refs: None,
        }),
        (None, None) => Err(ErrorBody {
            code: "creator_missing",
            message: "either userId or groupId is required".into(),
            status: None,
            detail: None,

            refs: None,
        }),
    }
}

/// Resolve assetType and content-type, or explain why it could not.
async fn resolve_asset_type(
    file: &std::path::Path,
    asset_type_override: Option<&str>,
    command: &str,
) -> Result<(String, &'static str, Option<String>), ErrorBody> {
    match asset_type_override {
        Some(t) => match content_type_for(t, file) {
            Some(ct) => Ok((t.to_string(), ct, None)),
            None => Err(ErrorBody {
                code: "asset_type_unknown",
                message: format!("cannot map --asset-type {t} to a content-type"),
                status: None,
                detail: Some(format!(
                    "the extension of {} is not known for {t}",
                    file.display()
                )),

                refs: None,
            }),
        },
        None => match infer_asset_from_file(file).await {
            Ok((t, ct, note)) => Ok((t.to_string(), ct, note)),
            Err(mut err) => {
                if err.code == "file_type_unknown" {
                    err.detail = Some(format!(
                        "{command} needs --asset-type when the extension is not recognized"
                    ));
                }
                Err(err)
            }
        },
    }
}

fn build_update_mask(name: bool, desc: bool, icon: bool, preview: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
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
        parts.push("previews");
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

        refs: None,
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

            refs: None,
        })?;
    Ok(reqwest::multipart::Form::new()
        .text("request", request.to_string())
        .part("fileContent", part))
}

/// Shared handling for endpoints that return an Operation.
async fn submit_operation(
    ctx: &Ctx,
    req: reqwest::RequestBuilder,
    command: &str,
    asset_type: &str,
    type_note: Option<String>,
) -> Output {
    let data = match ctx.send(req).await {
        Ok(resp) => match ctx.json_or_error(resp).await {
            Ok(data) => data,
            Err(err) => return failure(command, err),
        },
        Err(err) => return failure(command, err),
    };
    operation_output(data, command, asset_type, type_note)
}

fn operation_output(
    data: Value,
    command: &str,
    asset_type: &str,
    type_note: Option<String>,
) -> Output {
    let op = data.get("path").and_then(Value::as_str).unwrap_or("");
    let op_id = match normalize_operation_id(op) {
        Ok(id) => id,
        Err(err) => return failure(command, err),
    };
    if let Some(error) = data.get("error").filter(|error| !error.is_null()) {
        let mut out = failure(
            command,
            ErrorBody {
                code: "operation_failed",
                message: format!("Operation {op_id} failed"),
                status: None,
                detail: Some(error.to_string()),
                refs: None,
            },
        );
        out.data = Some(json!({"operationId": op_id, "assetType": asset_type, "response": data}));
        return out.with_hints(type_note.into_iter().chain([format!(
            "Inspect `rbxoc asset operation --operation-id {op_id}` and the existing asset before starting another upload; do not blindly repeat the mutation"
        )]));
    }
    Output::success(
        command,
        format!("Accepted the {asset_type} operation (operation {op_id})"),
        Some(json!({"operationId": op_id, "assetType": asset_type, "response": data})),
    ).with_hints(type_note.into_iter()
        .chain([
            format!("Run `rbxoc asset operation --operation-id {op_id}` to check completion"),
            "Processing is asynchronous; it is not immediate".into(),
        ])
        .chain((asset_type == "Decal").then(|| "The completed assetId is a Decal ID, not a resolved Image ID. For ImageLabel/ImageButton, see `asset image-id` and docs/image-ids.md. Do not re-upload to resolve it.".into()))
    )
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
                .as_array()
                .or_else(|| data.get("assetVersions").and_then(Value::as_array))
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
    const COMMAND: &str = "asset rollback";
    let path = format!("/assets/v1/assets/{asset_id}/versions:rollback");
    // Follow the formal multipart request schema (the embedded JSON curl
    // sample in OpenAPI currently disagrees). The resource uses the same
    // asset ID as the route; a bare version number is not a resource path.
    let form = reqwest::multipart::Form::new().text(
        "assetVersion",
        format!("assets/{asset_id}/versions/{version}"),
    );
    let req = ctx.client.post(ctx.url(&path)).multipart(form);
    let data = match ctx.send(req).await {
        Ok(resp) => match ctx.json_or_error(resp).await {
            Ok(data) => data,
            Err(err) => return failure(COMMAND, err),
        },
        Err(err) => return failure(COMMAND, err),
    };
    let prefix = format!("assets/{asset_id}/versions/");
    if !data
        .get("path")
        .and_then(Value::as_str)
        .is_some_and(|path| {
            path.strip_prefix(&prefix)
                .is_some_and(|version| version.parse::<u64>().is_ok_and(|v| v > 0))
        })
    {
        return failure(COMMAND, ErrorBody {
            code: "invalid_asset_version",
            message: "Rollback response is missing an AssetVersion resource for the target asset".into(),
            status: None,
            detail: Some("Inspect the asset's versions before repeating the rollback; it was sent at most once".into()),
            refs: None,
        });
    }
    Output::success(
        COMMAND,
        format!("Rolled asset {asset_id} back to version {version}"),
        Some(data),
    )
    .with_hints([format!(
        "Run `rbxoc asset versions --asset-id {asset_id}` to inspect the versions"
    )])
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

/// Start an asset create upload and return the operation id.
pub(crate) async fn start_create(
    ctx: &Ctx,
    file: &std::path::Path,
    display_name: &str,
    description: &str,
    creator_user_id: Option<u64>,
    creator_group_id: Option<u64>,
    asset_type_override: Option<&str>,
) -> Result<(String, String), ErrorBody> {
    let (asset_type, content_type, _type_note) =
        resolve_asset_type(file, asset_type_override, CMD_CREATE).await?;
    let creator = creator_json(creator_user_id, creator_group_id)?;
    let request = json!({
        "assetType": asset_type,
        "displayName": display_name,
        "description": description,
        "creationContext": { "creator": creator },
    });
    let form = build_multipart(&request, file, content_type, CMD_CREATE).await?;
    let req = ctx
        .client
        .post(ctx.url("/assets/v1/assets"))
        .multipart(form);
    let op_id = start_operation(ctx, req, CMD_CREATE).await?;
    Ok((op_id, asset_type))
}

/// Start a content-only in-place Model update. No name/description changes and
/// no automatic retry; the registry owns intent, operation and completion receipts.
pub(crate) async fn start_update(
    ctx: &Ctx,
    asset_id: u64,
    file: &std::path::Path,
) -> Result<String, ErrorBody> {
    let (asset_type, content_type, _) = resolve_asset_type(file, None, CMD_UPDATE).await?;
    if asset_id == 0 || !is_updateable_body(&asset_type) {
        return Err(ErrorBody {
            code: "asset_update_unsupported",
            message: "In-place content updates require a positive registered Model asset ID".into(),
            status: None,
            detail: None,
            refs: None,
        });
    }
    // Roblox's documented content-only update carries only assetId and omits
    // updateMask (which selects metadata fields, not the content body).
    let request = json!({ "assetId": asset_id.to_string() });
    let form = build_multipart(&request, file, content_type, CMD_UPDATE).await?;
    let req = ctx
        .client
        .patch(ctx.url(&format!("/assets/v1/assets/{asset_id}")))
        .multipart(form);
    start_operation(ctx, req, CMD_UPDATE).await
}

/// Poll until the operation completes or times out.
pub(crate) async fn wait_for_operation(
    ctx: &Ctx,
    operation_id: &str,
    max_attempts: u32,
    pause_ms: u64,
) -> Result<Value, ErrorBody> {
    let path = format!(
        "/assets/v1/operations/{}",
        crate::ctx::path_segment(operation_id)?
    );
    for attempt in 0..max_attempts {
        let data = ctx.get_json(&path).await?;
        let done = data.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
        if done {
            if let Some(err) = data.get("error") {
                return Err(ErrorBody {
                    code: "operation_failed",
                    message: format!("operation {operation_id} failed"),
                    status: None,
                    detail: Some(err.to_string()),
                    refs: None,
                });
            }
            return Ok(data);
        }
        if attempt + 1 < max_attempts {
            tokio::time::sleep(std::time::Duration::from_millis(pause_ms)).await;
        }
    }
    Err(ErrorBody {
        code: "operation_timeout",
        message: format!("operation {operation_id} did not finish in time"),
        status: None,
        detail: Some("Poll again with `rbxoc asset operation`".into()),
        refs: None,
    })
}

/// Read the created asset id from a completed operation payload.
pub fn asset_id_from_operation(data: &Value) -> Result<u64, ErrorBody> {
    if let Some(response) = data.get("response") {
        if let Some(id) = response
            .get("assetId")
            .and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| v.as_u64())
            })
            .filter(|id| *id > 0)
        {
            return Ok(id);
        }
        if let Some(path) = response.get("path").and_then(|v| v.as_str())
            && let Some(id) = path
                .strip_prefix("assets/")
                .and_then(|s| s.parse().ok())
                .filter(|id| *id > 0)
        {
            return Ok(id);
        }
    }
    Err(ErrorBody {
        code: "asset_id_missing",
        message: "completed operation has no assetId in response".into(),
        status: None,
        detail: Some(data.to_string()),
        refs: None,
    })
}

async fn start_operation(
    ctx: &Ctx,
    req: reqwest::RequestBuilder,
    _command: &str,
) -> Result<String, ErrorBody> {
    let resp = ctx.send_once(req).await?;
    if !resp.status().is_success() {
        return Err(crate::auth::error_from_response(resp).await);
    }
    let data = resp.json::<Value>().await.map_err(|e| ErrorBody {
        code: "invalid_json",
        message: format!("cannot parse the response: {e}"),
        status: None,
        detail: None,
        refs: None,
    })?;
    let path = data.get("path").and_then(|v| v.as_str()).unwrap_or("");
    normalize_operation_id(path)
}

/// Reject absent/malformed operation receipts rather than poll an invented ID.
pub fn normalize_operation_id(path: &str) -> Result<String, ErrorBody> {
    let id = path.strip_prefix("operations/").unwrap_or(path);
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ErrorBody {
            code: "operation_id_missing",
            message: "publication response has no valid operation ID; outcome may be uncertain"
                .into(),
            status: None,
            detail: None,
            refs: None,
        });
    }
    Ok(id.to_string())
}

/// Explicit offline fallback: interpret a Decal.Texture obtained in Studio.
/// Open Cloud does not provide a supported Decal-to-Image lookup here. Never
/// pretend the supplied texture was remotely verified, or replace the Decal ID.
pub fn image_id(decal_id: u64, texture: Option<&str>) -> Output {
    const COMMAND: &str = "asset image-id";
    if decal_id == 0 {
        return failure(
            COMMAND,
            ErrorBody {
                code: "asset_id_invalid",
                message: "--decal-id must be positive".into(),
                status: None,
                detail: None,
                refs: None,
            },
        );
    }
    let image_id = texture.and_then(parse_texture_id);
    let Some(image_id) = image_id else {
        let mut out = failure(COMMAND, ErrorBody {
            code: if texture.is_some() { "image_texture_invalid" } else { "image_resolution_unavailable" },
            message: format!("Image ID for Decal {decal_id} was not resolved; original Decal ID remains unchanged"),
            status: None, detail: Some("No supported Open Cloud Decal.Texture lookup is implemented. In Studio, load the Decal using InsertService:LoadAsset, read Decal.Texture, and pass it with --texture. See docs/image-ids.md.".into()), refs: None,
        });
        out.data = Some(json!({"decalId": decal_id, "imageId": null, "resolution": "unresolved"}));
        return out.with_hints(vec!["Do not upload again. Preserve the original operation/asset receipt; resolve its existing Decal.Texture in Studio.".into()]);
    };
    Output::success(COMMAND, format!("Read Image {image_id} from supplied Decal.Texture; Decal {decal_id} preserved"),
        Some(json!({"decalId": decal_id, "imageId": image_id, "uri": format!("rbxassetid://{image_id}"), "resolution": "supplied-studio-texture", "networkValidated": false})))
        .with_hints(vec!["Offline interpretation only: caller must verify the supplied Texture belongs to this Decal. Register with --id DECAL_ID --image-id IMAGE_ID; verify ImageLabel.IsLoaded in Studio.".into()])
}

fn parse_texture_id(texture: &str) -> Option<u64> {
    let id = if let Some(id) = texture.strip_prefix("rbxassetid://") {
        id.parse().ok()?
    } else {
        let url = reqwest::Url::parse(texture).ok()?;
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(url.host_str(), Some("www.roblox.com" | "roblox.com"))
            || !matches!(url.path(), "/asset" | "/asset/")
        {
            return None;
        }
        let ids: Vec<_> = url.query_pairs().filter(|(key, _)| key == "id").collect();
        if ids.len() != 1 {
            return None;
        }
        ids[0].1.parse().ok()?
    };
    (id > 0).then_some(id)
}

/// Check an operation's status.
pub async fn operation(ctx: &Ctx, operation_id: &str) -> Output {
    let path = match crate::ctx::path_segment(operation_id) {
        Ok(value) => format!("/assets/v1/operations/{value}"),
        Err(err) => return failure("asset operation", err),
    };
    match ctx.get_json(&path).await {
        Ok(data) => {
            let done = data.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
            if let Some(error) = data.get("error").filter(|error| !error.is_null()) {
                return failure(
                    "asset operation",
                    ErrorBody {
                        code: "operation_failed",
                        message: format!("Operation {operation_id} failed"),
                        status: None,
                        detail: Some(error.to_string()),
                        refs: None,
                    },
                )
                .with_hints([
                    "Inspect the operation error and existing asset before starting another upload"
                        .into(),
                ]);
            }
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

/// Report how a file would be classified, without uploading.
///
/// This is the credential-free path. It exists so the asset type of a
/// `.rbxm` / `.rbxmx` can be checked before spending an upload.
#[allow(clippy::too_many_arguments)]
pub async fn preview_create(
    file: &std::path::Path,
    display_name: &str,
    description: &str,
    creator_user_id: Option<u64>,
    creator_group_id: Option<u64>,
    asset_type_override: Option<&str>,
) -> Output {
    const COMMAND: &str = "asset create (dry-run)";
    let (asset_type, content_type, type_note) =
        match resolve_asset_type(file, asset_type_override, COMMAND).await {
            Ok(v) => v,
            Err(err) => return failure(COMMAND, err),
        };

    if let Err(err) = creator_json(creator_user_id, creator_group_id) {
        return failure(COMMAND, err);
    }

    let size = tokio::fs::metadata(file)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let mut hints = vec![
        "Drop --dry-run to upload for real".into(),
        "Set RBXOC_API_KEY before uploading".into(),
    ];
    if let Some(note) = &type_note {
        hints.insert(0, note.clone());
    }

    Output::success(
        COMMAND,
        format!("{asset_type} would be uploaded as {content_type}"),
        Some(serde_json::json!({
            "assetType": asset_type,
            "contentType": content_type,
            "displayName": display_name,
            "description": description,
            "bytes": size,
            "resolutionNote": type_note,
        })),
    )
    .with_hints(hints)
}

/// Report how an update would be classified, without uploading.
#[allow(clippy::too_many_arguments)]
pub async fn preview_update(
    file: Option<&std::path::Path>,
    asset_id: u64,
    asset_type: Option<&str>,
    display_name: Option<&str>,
    description: Option<&str>,
    icon: Option<&str>,
    previews: &[String],
) -> Output {
    const COMMAND: &str = "asset update (dry-run)";
    let plan = match plan_update(
        asset_id,
        file,
        asset_type,
        display_name,
        description,
        icon,
        previews,
    )
    .await
    {
        Ok(plan) => plan,
        Err(err) => return failure(COMMAND, err),
    };
    let size = if let Some(file) = file {
        match tokio::fs::metadata(file).await {
            Ok(metadata) => metadata.len(),
            Err(err) => {
                return failure(
                    COMMAND,
                    ErrorBody {
                        code: "file_unreadable",
                        message: format!("Cannot read {}", file.display()),
                        status: None,
                        detail: Some(err.to_string()),
                        refs: None,
                    },
                );
            }
        }
    } else {
        0
    };
    let mut hints = vec![
        "Drop --dry-run to update for real".into(),
        "Set RBXOC_API_KEY before updating".into(),
        "Icon/preview references must already be Image assets; icons must be square. This plan does not verify their type, ownership or dimensions.".into(),
    ];
    if let Some(note) = &plan.type_note {
        hints.insert(0, note.clone());
    }
    Output::success(
        COMMAND,
        if file.is_some() {
            "Model content would be updated"
        } else {
            "Asset metadata would be updated without a file"
        },
        Some(json!({
            "assetId": asset_id, "assetType": plan.asset_type, "contentType": plan.content_type,
            "bytes": size, "updateMask": plan.mask, "request": plan.request,
            "resolutionNote": plan.type_note,
        })),
    )
    .with_hints(hints)
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
    fn creator_json_rejects_ambiguous_and_missing() {
        assert!(creator_json(Some(1), None).is_ok());
        assert!(creator_json(None, Some(2)).is_ok());
        assert_eq!(
            creator_json(Some(1), Some(2)).unwrap_err().code,
            "creator_ambiguous"
        );
        assert_eq!(
            creator_json(None, None).unwrap_err().code,
            "creator_missing"
        );
    }

    #[test]
    fn parses_asset_id_from_operation_response() {
        let data = json!({
            "done": true,
            "response": { "path": "assets/12345", "assetId": "12345" }
        });
        assert_eq!(asset_id_from_operation(&data).unwrap(), 12345);
    }

    #[test]
    fn update_mask_is_minimal() {
        assert_eq!(build_update_mask(false, false, false, false), "");
        assert_eq!(
            build_update_mask(true, true, true, true),
            "displayName,description,icon,previews"
        );
    }

    #[test]
    fn distinguishes_decal_from_supplied_image_without_claiming_remote_resolution() {
        for texture in [
            "rbxassetid://456",
            "http://www.roblox.com/asset/?id=456",
            "https://roblox.com/asset?id=456",
        ] {
            let out = image_id(123, Some(texture));
            assert!(out.ok, "{out:?}");
            let data = out.data.unwrap();
            assert_eq!(data["decalId"], 123);
            assert_eq!(data["imageId"], 456);
            assert_eq!(data["networkValidated"], false);
            assert_eq!(data["resolution"], "supplied-studio-texture");
        }
    }

    #[test]
    fn failed_image_resolution_preserves_original_id_and_returns_fallback() {
        let unavailable = image_id(123, None);
        assert_eq!(
            unavailable.error.unwrap().code,
            "image_resolution_unavailable"
        );
        assert_eq!(unavailable.data.unwrap()["decalId"], 123);
        for texture in [
            "rbxassetid://0",
            "https://example.com/asset/?id=456",
            "not a texture",
            "https://www.roblox.com/asset/?id=1&id=2",
        ] {
            let out = image_id(123, Some(texture));
            assert!(!out.ok);
            assert_eq!(out.error.unwrap().code, "image_texture_invalid");
            assert!(
                out.hints
                    .iter()
                    .any(|hint| hint.contains("Do not upload again"))
            );
            assert_eq!(out.data.unwrap()["imageId"], serde_json::Value::Null);
        }
    }

    #[test]
    fn rejects_invalid_operation_and_zero_asset_receipts() {
        assert_eq!(
            normalize_operation_id("operations/abc-123_def").unwrap(),
            "abc-123_def"
        );
        for id in ["", "(unknown)", "../operation", "op?query", "operations/"] {
            assert!(normalize_operation_id(id).is_err());
        }
        assert!(asset_id_from_operation(&json!({"response": {"assetId": "0"}})).is_err());
        assert!(asset_id_from_operation(&json!({"response": {"path": "assets/0"}})).is_err());
    }
}
