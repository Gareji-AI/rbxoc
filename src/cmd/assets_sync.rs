//! Create missing assets or explicitly update changed Models with durable receipts.

use std::path::Path;

use anyhow::Context;
use serde_json::json;

use crate::cmd::{asset, assets, model, preprocess};
use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

const COMMAND: &str = "assets sync";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    CreateMissing,
    UpdateChanged,
}

/// Explicit offline recovery of an operation ID learned from a saved response
/// or Creator Dashboard. Never creates a publication or fabricates freshness.
pub fn recover_operation(dir: &Path, path: &str, operation_id: &str) -> Output {
    const RECOVER: &str = "assets recover";
    let result = (|| -> anyhow::Result<(String, bool)> {
        let _guard = assets::registry_guard(dir)?;
        let manifest = assets::load_manifest(dir)?;
        let mut lock = assets::load_lockfile(dir)?;
        let key = path.replace('\\', "/");
        let sources = assets::collect_sources(dir, &manifest)?;
        let source = sources
            .get(&key)
            .with_context(|| format!("{key}: not a manifest key"))?;
        let pending = lock
            .pending
            .get_mut(&key)
            .with_context(|| format!("{key}: no pending upload receipt to recover"))?;
        anyhow::ensure!(
            pending.fingerprint == assets::fingerprint(source)?
                && pending.creator == manifest.creator,
            "{key}: pending upload belongs to different input bytes, preprocessing settings, or creator; restore originals first"
        );
        let update = pending.target_asset_id.is_some();
        if let Some(target) = pending.target_asset_id {
            anyhow::ensure!(
                lock.assets.get(&key) == Some(&target) && target > 0,
                "{key}: registered ID no longer matches the pending update target"
            );
        }
        let id =
            asset::normalize_operation_id(operation_id).map_err(|e| anyhow::anyhow!(e.message))?;
        anyhow::ensure!(
            pending.operation_id.as_ref().is_none_or(|old| old == &id),
            "{key}: an existing operation is already recorded; refusing to replace its identity"
        );
        pending.operation_id = Some(id.clone());
        assets::save_lockfile(dir, &lock)?;
        Ok((id, update))
    })();
    match result {
        Ok((id, update)) => Output::success(RECOVER, format!("Recorded operation {id} for {path}; no cloud writes"), Some(json!({"path": path, "operationId": id, "update": update})))
            .with_hints(vec![format!("Run sync {}with --path {path} to poll this operation, not submit it again. Only attach an operation known to belong to this exact source, creator and asset ID.", if update { "--update " } else { "" })]),
        Err(e) => failure(RECOVER, local_error("recovery_error", format!("{e:#}"))),
    }
}

/// Freshness check only. No API credentials required.
pub fn run_dry_run(dir: &Path, selection: &[String]) -> Output {
    dry_run_report(dir, selection)
}

/// Upload missing files, update the lockfile, and write generated source.
pub async fn run(ctx: &Ctx, dir: &Path, selection: &[String]) -> Output {
    run_mode(ctx, dir, selection, Mode::CreateMissing).await
}

/// Explicitly update changed, receipt-backed Models without replacing IDs.
pub async fn run_update(ctx: &Ctx, dir: &Path, selection: &[String]) -> Output {
    run_mode(ctx, dir, selection, Mode::UpdateChanged).await
}

async fn run_mode(ctx: &Ctx, dir: &Path, selection: &[String], mode: Mode) -> Output {
    if mode == Mode::UpdateChanged && selection.is_empty() {
        return failure(
            COMMAND,
            local_error(
                "update_selection_required",
                "--update requires at least one explicit --path".into(),
            ),
        );
    }
    let _guard = match assets::registry_guard(dir) {
        Ok(guard) => guard,
        Err(e) => return registry_failure(e),
    };
    let (manifest, mut lock) = match load_registry(dir) {
        Ok(v) => v,
        Err(out) => return *out,
    };
    let sources = match assets::collect_sources(dir, &manifest) {
        Ok(sources) => sources,
        Err(e) => return registry_failure(e),
    };
    let paths: Vec<_> = sources.keys().cloned().collect();
    let selected = match assets::select_paths(&paths, selection) {
        Ok(selected) => selected,
        Err(e) => return registry_failure(e),
    };
    let missing = assets::missing_ids(&selected, &lock);
    let mut prospective: std::collections::BTreeSet<_> =
        lock.assets.keys().map(String::as_str).collect();
    prospective.extend(missing.iter().map(String::as_str));
    if let Err(e) =
        crate::cmd::codegen::validate_keys(prospective, &assets::codegen_options(&manifest.codegen))
    {
        return registry_failure(e);
    }

    let prepared = match preflight(dir, &manifest, &lock, &sources, &selected, mode, true).await {
        Ok(prepared) => prepared,
        Err(err) => return failure(COMMAND, err),
    };
    let warnings = prepared
        .iter()
        .flat_map(|item| item.warnings.iter().cloned())
        .collect::<Vec<_>>();
    let outcome = async {
        let (user_id, group_id) = creator_ids(&manifest);
        let mut uploaded = 0usize;
        let mut updated = 0usize;
        let mut resumed = 0usize;
        for item in prepared {
            let key = &item.key;
            match assets::fingerprint(&item.source) {
                Ok(current) if current == item.fingerprint => {}
                Ok(_) => {
                    return failure(
                        COMMAND,
                        local_error(
                            "source_changed",
                            format!(
                                "{key}: source changed after preflight; no publication was attempted for this file"
                            ),
                        ),
                    );
                }
                Err(e) => return registry_failure(e),
            }
            let op_id = if let Some(pending) = lock.pending.get(key) {
                resumed += 1;
                // Preflight already rejected uncertain or mismatched receipts.
                pending.operation_id.clone().expect("validated operation")
            } else {
                lock.pending.insert(
                    key.clone(),
                    assets::PendingUpload {
                        fingerprint: item.fingerprint.clone(),
                        creator: manifest.creator.clone(),
                        asset_type: item.asset_type.clone(),
                        target_asset_id: item.target_asset_id,
                        operation_id: None,
                    },
                );
                if let Err(e) = assets::save_lockfile(dir, &lock) {
                    return receipt_failure(key, None, e);
                }
                let display_name = item
                    .source
                    .path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(key);
                let started = match item.target_asset_id {
                    Some(id) => asset::start_update(ctx, id, item.upload.as_path()).await,
                    None => asset::start_create(
                        ctx,
                        item.upload.as_path(),
                        display_name,
                        "",
                        user_id,
                        group_id,
                        None,
                    )
                    .await
                    .map(|(id, _)| id),
                };
                let op_id = match started {
                    Ok(op_id) => op_id,
                    Err(err) => {
                        // An explicit 4xx rejection (except timeout) did not accept
                        // creation. Transport, 5xx, and invalid/lost response remain
                        // uncertain and must not be retried as a new create.
                        if err
                            .status
                            .is_some_and(|status| (400..500).contains(&status) && status != 408)
                        {
                            lock.pending.remove(key);
                            if let Err(e) = assets::save_lockfile(dir, &lock) {
                                return receipt_failure(key, None, e);
                            }
                            return source_failure(key, err);
                        }
                        return failure(
                            COMMAND,
                            local_error(
                                "publication_uncertain",
                                format!("{key}: publication outcome is unknown: {}", err.message),
                            ),
                        )
                        .with_hints(recovery_hints(key, None));
                    }
                };
                lock.pending
                    .get_mut(key)
                    .expect("persisted intent")
                    .operation_id = Some(op_id.clone());
                if let Err(e) = assets::save_lockfile(dir, &lock) {
                    return receipt_failure(key, Some(&op_id), e);
                }
                if item.target_asset_id.is_some() {
                    updated += 1;
                } else {
                    uploaded += 1;
                }
                op_id
            };
            let done = match asset::wait_for_operation(ctx, &op_id, 60, 2000).await {
                Ok(v) => v,
                Err(err) => {
                    return source_failure(key, err).with_hints(recovery_hints(key, Some(&op_id)));
                }
            };
            let asset_id = match asset::asset_id_from_operation(&done) {
                Ok(id) => id,
                Err(err) => {
                    return source_failure(key, err).with_hints(recovery_hints(key, Some(&op_id)));
                }
            };
            if let Some(target) = item.target_asset_id
                && asset_id != target
            {
                return failure(COMMAND, local_error("asset_id_mismatch", format!(
                    "{key}: update receipt returned asset {asset_id}, expected {target}; original ID and pending operation preserved"
                ))).with_hints(recovery_hints(key, Some(&op_id)));
            }
            lock.assets.insert(key.clone(), asset_id);
            lock.publications.insert(
                key.clone(),
                assets::PublicationReceipt {
                    asset_id,
                    fingerprint: item.fingerprint,
                    creator: manifest.creator.clone(),
                    asset_type: item.asset_type,
                },
            );
            lock.pending.remove(key);
            if let Err(e) = assets::save_lockfile(dir, &lock) {
                return receipt_failure(key, Some(&op_id), e).with_hints(vec![format!(
                    "Completed asset ID: {asset_id}; preserve this receipt before any retry."
                )]);
            }
        }

        let out_file = match assets::write_generated(dir, &manifest, &lock) {
            Ok(p) => p,
            Err(e) => return registry_failure(e),
        };
        let freshness = match assets::freshness(&sources, &selected, &manifest, &lock) {
            Ok(state) => state,
            Err(e) => return registry_failure(e),
        };

        let unresolved_images: Vec<_> = lock
            .assets
            .iter()
            .filter(|(key, id)| {
                assets::is_image_entry(key, &lock)
                    && !lock
                        .image_ids
                        .get(*key)
                        .is_some_and(|image| image.decal_id == **id && image.image_id > 0)
            })
            .map(|(key, _)| key.clone())
            .collect();
        Output::success(
            COMMAND,
            format!(
                "Synced {uploaded} uploads, {updated} updates; wrote {} ({} registered)",
                out_file.display(),
                lock.assets.len()
            ),
            Some(json!({
                "uploaded": uploaded,
                "updated": updated,
                "resumed": resumed,
                "output": out_file.to_string_lossy(),
                "registered": lock.assets.len(),
                "missingAfter": assets::missing_ids(&paths, &lock),
                "selected": selected,
                "current": freshness.current,
                "changed": freshness.changed,
                "unverified": freshness.unverified,
                "unresolvedImages": unresolved_images,
            })),
        )
        .with_hints(vec!["Registered IDs were preserved. Updates require explicit --update with --path and a changed Model receipt; unverified files are never updated automatically.".into()])
        .with_hints((!unresolved_images.is_empty()).then(|| "Image uploads return Decal IDs, not resolved Image IDs. Preserve these receipts; resolve Decal.Texture in Studio, register --id DECAL_ID --image-id IMAGE_ID, then enable codegen.image_ids for image consumers. See docs/image-ids.md.".into()))
    }
    .await;
    outcome.with_hints(warnings)
}

struct Prepared {
    key: String,
    source: assets::Source,
    fingerprint: String,
    upload: preprocess::UploadFile,
    asset_type: String,
    target_asset_id: Option<u64>,
    warnings: Vec<String>,
}

/// Validate the complete update selection before any upload preparation or call.
fn validate_updates(
    manifest: &assets::Manifest,
    lock: &assets::Lockfile,
    sources: &std::collections::BTreeMap<String, assets::Source>,
    selected: &[String],
) -> Result<(), ErrorBody> {
    if selected.is_empty() {
        return Err(local_error(
            "update_selection_required",
            "--update requires at least one explicit --path".into(),
        ));
    }
    let mut targets = std::collections::BTreeSet::new();
    for key in selected {
        let id = lock.assets.get(key).filter(|id| **id > 0).ok_or_else(|| {
            local_error(
                "asset_update_not_changed",
                format!("{key}: --update refuses missing entries; use normal sync to create them"),
            )
        })?;
        let receipt = lock.publications.get(key).filter(|r| r.asset_id == *id).ok_or_else(|| local_error(
            "asset_update_not_changed", format!("{key}: --update refuses unverified entries; a successful publication receipt is required")
        ))?;
        if receipt.asset_type != "Model" {
            return Err(local_error(
                "asset_update_unsupported",
                format!(
                    "{key}: {} does not support in-place content updates; only Model is supported. Use an explicit replacement workflow, never an automatic create.",
                    receipt.asset_type
                ),
            ));
        }
        if receipt.creator != manifest.creator {
            return Err(local_error(
                "update_creator_changed",
                format!(
                    "{key}: restore the original receipt's creator before updating its asset ID"
                ),
            ));
        }
        if !targets.insert(*id) {
            return Err(local_error(
                "update_target_collision",
                format!(
                    "{key}: more than one selected source targets asset {id}; select one source per asset ID"
                ),
            ));
        }
        if let Some(pending) = lock.pending.get(key) {
            if pending.target_asset_id != Some(*id) {
                return Err(local_error(
                    "recovery_input_changed",
                    format!("{key}: pending publication does not target the registered asset ID"),
                ));
            }
        } else if receipt.fingerprint
            == assets::fingerprint(&sources[key])
                .map_err(|e| local_error("file_unreadable", format!("{key}: {e:#}")))?
        {
            return Err(local_error(
                "asset_update_not_changed",
                format!(
                    "{key}: --update refuses current entries; source bytes or preprocessing must have changed"
                ),
            ));
        }
    }
    Ok(())
}

/// Credential-free planning, including update eligibility/type/size checks.
/// Unlike real preflight, this does not persist intent or touch output paths.
pub async fn preview_update(dir: &Path, selection: &[String]) -> Output {
    let result = async {
        if selection.is_empty() {
            return Err(local_error(
                "update_selection_required",
                "--update requires explicit --path".into(),
            ));
        }
        let manifest = assets::load_manifest(dir)
            .map_err(|e| local_error("registry_error", format!("{e:#}")))?;
        let lock = assets::load_lockfile(dir)
            .map_err(|e| local_error("registry_error", format!("{e:#}")))?;
        let sources = assets::collect_sources(dir, &manifest)
            .map_err(|e| local_error("registry_error", format!("{e:#}")))?;
        let paths = sources.keys().cloned().collect::<Vec<_>>();
        let selected = assets::select_paths(&paths, selection)
            .map_err(|e| local_error("registry_error", format!("{e:#}")))?;
        let prepared = preflight(
            dir,
            &manifest,
            &lock,
            &sources,
            &selected,
            Mode::UpdateChanged,
            false,
        )
        .await?;
        Ok(prepared.iter().map(|item| json!({
            "path": item.key, "assetId": item.target_asset_id,
            "action": if lock.pending.contains_key(&item.key) { "resume-update" } else { "update" },
            "assetType": item.asset_type,
        })).collect::<Vec<_>>())
    }
    .await;
    match result {
        Ok(updates) => Output::success(COMMAND, format!("Planned {} in-place Model updates; no cloud calls", updates.len()), Some(json!({"updates": updates, "dryRun": true})))
            .with_hints(vec!["IDs remain unchanged. This checks local eligibility, type and size, not cloud state, output writability, moderation or delivery.".into()]),
        Err(err) => failure(COMMAND, err),
    }
}

/// Prepare the entire selection, one file in memory at a time. Converted files
/// stay on disk under RAII guards until publication, including on early errors.
async fn preflight(
    dir: &Path,
    manifest: &assets::Manifest,
    lock: &assets::Lockfile,
    sources: &std::collections::BTreeMap<String, assets::Source>,
    selected: &[String],
    mode: Mode,
    persist: bool,
) -> Result<Vec<Prepared>, ErrorBody> {
    if mode == Mode::UpdateChanged {
        validate_updates(manifest, lock, sources, selected)?;
    }
    assets::render(manifest, lock).map_err(|e| local_error("registry_error", format!("{e:#}")))?;
    let mut prepared = Vec::new();
    for key in selected {
        let source = &sources[key];
        let fingerprint = assets::fingerprint(source)
            .map_err(|e| local_error("file_unreadable", format!("{key}: {e:#}")))?;
        let upload =
            preprocess::prepare_upload_file(&source.path, source.bleed).map_err(|mut err| {
                err.message = format!("{key}: {}", err.message);
                err
            })?;
        let size = std::fs::metadata(upload.as_path())
            .map_err(|e| local_error("file_unreadable", format!("{key}: {e}")))?
            .len();
        if size == 0 || size > 20 * 1024 * 1024 {
            return Err(local_error(
                "file_size_invalid",
                format!(
                    "{key}: prepared upload must be nonempty and at most 20 MiB, got {size} bytes"
                ),
            ));
        }
        let (asset_type, _, _) = asset::infer_asset_from_file(upload.as_path())
            .await
            .map_err(|mut err| {
                err.message = format!("{key}: {}", err.message);
                err
            })?;
        let target_asset_id = (mode == Mode::UpdateChanged).then(|| lock.assets[key]);
        if mode == Mode::UpdateChanged && asset_type != "Model" {
            return Err(local_error(
                "asset_update_unsupported",
                format!(
                    "{key}: prepared {asset_type} content cannot update a Model; restore Model content or use explicit replacement"
                ),
            ));
        }
        if let Some(pending) = lock.pending.get(key) {
            if pending.fingerprint != fingerprint
                || pending.creator != manifest.creator
                || pending.asset_type != asset_type
                || pending.target_asset_id != target_asset_id
            {
                return Err(local_error(
                    "recovery_input_changed",
                    format!(
                        "{key}: pending upload belongs to different source bytes, preprocessing settings, type, or creator; restore original inputs before recovery"
                    ),
                ));
            }
            if pending.operation_id.is_none() {
                return Err(local_error(
                    "publication_uncertain",
                    format!(
                        "{key}: a prior publication has no operation receipt; inspect Creator Dashboard, then use `assets recover --path {key} --operation-id ID` or explicitly register the known asset ID; do not blindly publish again"
                    ),
                ));
            }
            if let Some(id) = &pending.operation_id {
                let normalized = asset::normalize_operation_id(id)?;
                if normalized != *id {
                    return Err(local_error(
                        "recovery_error",
                        format!(
                            "{key}: stored operation ID is not normalized; reconcile its receipt before recovery"
                        ),
                    ));
                }
            }
            if lock.assets.contains_key(key) && mode == Mode::CreateMissing {
                return Err(local_error(
                    "registry_error",
                    format!(
                        "{key}: both registered and pending; reconcile the receipt before syncing"
                    ),
                ));
            }
        }
        if assets::fingerprint(source)
            .map_err(|e| local_error("file_unreadable", format!("{key}: {e:#}")))?
            != fingerprint
        {
            return Err(local_error(
                "source_changed",
                format!("{key}: source changed during preflight"),
            ));
        }
        if !lock.assets.contains_key(key) || mode == Mode::UpdateChanged {
            if manifest.codegen.image_ids && asset_type == "Decal" {
                return Err(local_error(
                    "image_resolution_required",
                    format!(
                        "{key}: image_ids codegen needs an explicitly resolved Image ID. First upload with codegen.image_ids = false, retain the Decal receipt, then resolve/register its Image ID and enable image_ids. No upload was attempted."
                    ),
                ));
            }
            let warnings = model::large_xml_animation_warning(upload.as_path(), asset_type, size)
                .into_iter()
                .collect();
            prepared.push(Prepared {
                key: key.clone(),
                source: source.clone(),
                fingerprint,
                upload,
                asset_type: asset_type.into(),
                target_asset_id,
                warnings,
            });
        }
    }
    // Check local destination/receipt persistence before mutating the cloud.
    if persist {
        assets::prepare_output_path(dir, manifest, sources)
            .map_err(|e| local_error("registry_error", format!("{e:#}")))?;
        assets::save_lockfile(dir, lock).map_err(|e| {
            local_error(
                "receipt_write_failed",
                format!("cannot persist registry before publication: {e:#}"),
            )
        })?;
    }
    Ok(prepared)
}

fn local_error(code: &'static str, message: String) -> ErrorBody {
    ErrorBody {
        code,
        message,
        status: None,
        detail: None,
        refs: None,
    }
}

fn source_failure(key: &str, mut err: ErrorBody) -> Output {
    err.message = format!("{key}: {}", err.message);
    failure(COMMAND, err)
}

fn recovery_hints(key: &str, operation_id: Option<&str>) -> Vec<String> {
    let mut hints = vec![format!(
        "Do not issue a new create or update for {key}. Keep the lockfile and restore original source/creator/target settings before retrying. For pending updates use sync --update --path {key}, selecting only unfinished paths."
    )];
    if let Some(id) = operation_id {
        hints.push(format!("Run `rbxoc asset operation --operation-id {id}`; sync will poll this existing operation on retry."));
        hints.push(format!("If the operation receipt was not saved, attach it with `assets recover --path {key} --operation-id {id}`."));
    } else {
        hints.push("Inspect Creator Dashboard for the outcome; attach a known operation with `assets recover`, or record a known successful ID with `assets register`. A lost response cannot guarantee exactly-once publication.".into());
    }
    hints
}

fn receipt_failure(key: &str, operation_id: Option<&str>, err: anyhow::Error) -> Output {
    failure(
        COMMAND,
        local_error(
            "receipt_write_failed",
            format!("{key}: cannot persist upload receipt: {err:#}"),
        ),
    )
    .with_hints(recovery_hints(key, operation_id))
}

fn dry_run_report(dir: &Path, selection: &[String]) -> Output {
    let (manifest, lock) = match load_registry(dir) {
        Ok(v) => v,
        Err(out) => return *out,
    };
    let sources = match assets::collect_sources(dir, &manifest) {
        Ok(sources) => sources,
        Err(e) => return registry_failure(e),
    };
    let paths: Vec<_> = sources.keys().cloned().collect();
    let paths = match assets::select_paths(&paths, selection) {
        Ok(selected) => selected,
        Err(e) => return registry_failure(e),
    };
    let warnings = paths
        .iter()
        .filter_map(|key| model::inspect_large_xml_animation(&sources[key].path))
        .collect::<Vec<_>>();
    let missing = assets::missing_ids(&paths, &lock);
    let freshness = match assets::freshness(&sources, &paths, &manifest, &lock) {
        Ok(state) => state,
        Err(e) => return registry_failure(e),
    };
    let pending: Vec<_> = paths
        .iter()
        .filter(|path| lock.pending.contains_key(*path))
        .collect();
    let data = json!({
        "total": paths.len(), "selected": paths, "missing": missing,
        "current": freshness.current, "changed": freshness.changed,
        "unverified": freshness.unverified, "pending": pending,
        "check": "registry-and-known-freshness-only",
        "warnings": warnings,
    });
    if missing.is_empty() && freshness.changed.is_empty() && pending.is_empty() {
        return Output::success(
            COMMAND,
            format!("All {} selected files have asset IDs ({} unverified)", paths.len(), freshness.unverified.len()),
            Some(data),
        ).with_hints(vec!["Registry check only with read-only XML advisories: unverified entries are not proof of freshness; no preprocessing or full upload preflight was performed.".into()])
        .with_hints(warnings);
    }
    failure(
        COMMAND,
        ErrorBody {
            code: "assets_out_of_sync",
            message: format!(
                "{} missing, {} changed, {} pending selected files",
                missing.len(),
                freshness.changed.len(),
                pending.len()
            ),
            status: None,
            detail: Some(data.to_string()),
            refs: None,
        },
    )
    .with_hints(vec![
        "Run without --dry-run to upload missing files or recover known operations. Changed existing files require an explicit update/replacement; unverified entries remain unverified.".into(),
    ])
    .with_hints(warnings)
}

/// Resolve a lock/manifest key to a source file and whether to alpha-bleed it.
pub fn resolve_file_for_key(
    dir: &Path,
    manifest: &assets::Manifest,
    key: &str,
) -> anyhow::Result<(std::path::PathBuf, bool)> {
    let source = assets::collect_sources(dir, manifest)?
        .remove(key)
        .with_context(|| format!("no manifest file matches lock key {key}"))?;
    Ok((source.path, source.bleed))
}

fn creator_ids(manifest: &assets::Manifest) -> (Option<u64>, Option<u64>) {
    match manifest.creator.kind.as_str() {
        "user" => (Some(manifest.creator.id), None),
        "group" => (None, Some(manifest.creator.id)),
        _ => (None, None),
    }
}

fn load_registry(dir: &Path) -> Result<(assets::Manifest, assets::Lockfile), Box<Output>> {
    let manifest = assets::load_manifest(dir).map_err(|e| Box::new(registry_failure(e)))?;
    let lock = assets::load_lockfile(dir).map_err(|e| Box::new(registry_failure(e)))?;
    Ok((manifest, lock))
}

fn registry_failure(err: anyhow::Error) -> Output {
    failure(
        COMMAND,
        ErrorBody {
            code: "registry_error",
            message: format!("{err:#}"),
            status: None,
            detail: None,
            refs: None,
        },
    )
}

#[cfg(test)]
#[path = "assets-sync-tests.rs"]
mod publication_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::assets::{CodegenConfig, Creator, Inputs, Lockfile, Manifest};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn creator_ids_from_manifest() {
        let group = Manifest {
            creator: Creator {
                kind: "group".into(),
                id: 99,
            },
            codegen: CodegenConfig {
                language: "luau".into(),
                style: "nested".into(),
                output_path: PathBuf::from("out"),
                variable: "Assets".into(),
                output_basename: None,
                generator: "rbxoc".into(),
                strip_extensions: false,
                content: false,
                image_ids: false,
                bleed: true,
            },
            inputs: BTreeMap::new(),
        };
        assert_eq!(creator_ids(&group), (None, Some(99)));
    }

    #[test]
    fn dry_run_flags_missing_without_upload() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = Manifest {
            creator: Creator {
                kind: "group".into(),
                id: 1,
            },
            codegen: CodegenConfig {
                language: "luau".into(),
                style: "nested".into(),
                output_path: PathBuf::from("out"),
                variable: "Assets".into(),
                output_basename: None,
                generator: "rbxoc".into(),
                strip_extensions: false,
                content: false,
                image_ids: false,
                bleed: true,
            },
            inputs: [(
                "main".into(),
                Inputs {
                    path: "assets/*.png".into(),
                    path_key_from: None,
                    bleed: None,
                },
            )]
            .into(),
        };
        std::fs::create_dir_all(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("assets/a.png"), b"png").unwrap();
        std::fs::write(
            dir.path().join(assets::MANIFEST_NAME),
            toml::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assets::save_lockfile(dir.path(), &Lockfile::new()).unwrap();

        let paths = assets::collect_paths(dir.path(), &manifest).unwrap();
        let missing = assets::missing_ids(&paths, &Lockfile::new());
        assert_eq!(missing, vec!["assets/a.png"]);
        assert!(!run_dry_run(dir.path(), &[]).ok);
        assert!(!run_dry_run(dir.path(), &["unknown.png".into()]).ok);

        std::fs::write(dir.path().join("assets/b.png"), b"other png").unwrap();
        let mut lock = Lockfile::new();
        lock.assets.insert("assets/a.png".into(), 10);
        assets::save_lockfile(dir.path(), &lock).unwrap();
        let scoped = run_dry_run(dir.path(), &["assets/a.png".into()]);
        assert!(scoped.ok);
        assert_eq!(scoped.data.unwrap()["unverified"], json!(["assets/a.png"]));
        assert!(!run_dry_run(dir.path(), &[]).ok);

        let source = assets::Source {
            path: dir.path().join("assets/a.png"),
            bleed: true,
        };
        lock.publications.insert(
            "assets/a.png".into(),
            assets::PublicationReceipt {
                asset_id: 10,
                fingerprint: assets::fingerprint(&source).unwrap(),
                creator: manifest.creator,
                asset_type: "Decal".into(),
            },
        );
        assets::save_lockfile(dir.path(), &lock).unwrap();
        assert!(run_dry_run(dir.path(), &["assets/a.png".into()]).ok);
        std::fs::write(&source.path, b"new content").unwrap();
        let stale = run_dry_run(dir.path(), &["assets/a.png".into()]);
        assert!(!stale.ok);
        assert!(stale.error.unwrap().detail.unwrap().contains("changed"));
        assert_eq!(
            assets::load_lockfile(dir.path()).unwrap().assets["assets/a.png"],
            10
        );
    }
}
