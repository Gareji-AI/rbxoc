//! Upload missing manifest files, refresh the lockfile, and run codegen.

use std::path::Path;

use anyhow::Context;
use serde_json::json;

use crate::cmd::{asset, assets, preprocess};
use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

const COMMAND: &str = "assets sync";

/// Freshness check only. No API credentials required.
pub fn run_dry_run(dir: &Path) -> Output {
    dry_run_report(dir)
}

/// Upload missing files, update the lockfile, and write generated source.
pub async fn run(ctx: &Ctx, dir: &Path) -> Output {
    let (manifest, mut lock) = match load_registry(dir) {
        Ok(v) => v,
        Err(out) => return *out,
    };
    let paths = match assets::collect_paths(dir, &manifest) {
        Ok(p) => p,
        Err(e) => return registry_failure(e),
    };
    let missing = assets::missing_ids(&paths, &lock);

    let (user_id, group_id) = creator_ids(&manifest);
    let mut uploaded = 0usize;
    for rel in &missing {
        let (file, bleed) = match resolve_file_for_key(dir, &manifest, rel) {
            Ok(p) => p,
            Err(e) => return registry_failure(e),
        };
        let upload = match preprocess::prepare_upload_file(&file, bleed) {
            Ok(u) => u,
            Err(err) => return failure(COMMAND, err),
        };
        let display_name = file.file_stem().and_then(|s| s.to_str()).unwrap_or(rel);
        let (op_id, _asset_type) = match asset::start_create(
            ctx,
            upload.as_path(),
            display_name,
            "",
            user_id,
            group_id,
            None,
        )
        .await
        {
            Ok(v) => v,
            Err(err) => return failure(COMMAND, err),
        };
        let done = match asset::wait_for_operation(ctx, &op_id, 60, 2000).await {
            Ok(v) => v,
            Err(err) => return failure(COMMAND, err),
        };
        let asset_id = match asset::asset_id_from_operation(&done) {
            Ok(id) => id,
            Err(err) => return failure(COMMAND, err),
        };
        lock.assets.insert(rel.clone(), asset_id);
        uploaded += 1;
    }

    if uploaded > 0 {
        if let Err(e) = assets::save_lockfile(dir, &lock) {
            return registry_failure(e);
        }
    }

    let out_file = match assets::write_generated(dir, &manifest, &lock) {
        Ok(p) => p,
        Err(e) => return registry_failure(e),
    };

    Output::success(
        COMMAND,
        format!(
            "Synced {uploaded} uploads; wrote {} ({} registered)",
            out_file.display(),
            lock.assets.len()
        ),
        Some(json!({
            "uploaded": uploaded,
            "output": out_file.to_string_lossy(),
            "registered": lock.assets.len(),
            "missingAfter": assets::missing_ids(&paths, &lock),
        })),
    )
}

fn dry_run_report(dir: &Path) -> Output {
    let (manifest, lock) = match load_registry(dir) {
        Ok(v) => v,
        Err(out) => return *out,
    };
    let paths = match assets::collect_paths(dir, &manifest) {
        Ok(p) => p,
        Err(e) => return registry_failure(e),
    };
    let missing = assets::missing_ids(&paths, &lock);
    if missing.is_empty() {
        return Output::success(
            COMMAND,
            format!("All {} manifest files have asset IDs", paths.len()),
            Some(json!({ "total": paths.len(), "missing": [] })),
        );
    }
    failure(
        COMMAND,
        ErrorBody {
            code: "assets_out_of_sync",
            message: format!("{} files are not registered yet", missing.len()),
            status: None,
            detail: Some(missing.join(", ")),
            refs: None,
        },
    )
    .with_hints(vec![
        "Run without --dry-run to upload and register them".into(),
    ])
}

/// Resolve a lock/manifest key to a source file and whether to alpha-bleed it.
pub fn resolve_file_for_key(
    dir: &Path,
    manifest: &assets::Manifest,
    key: &str,
) -> anyhow::Result<(std::path::PathBuf, bool)> {
    for input in manifest.inputs.values() {
        let bleed = input.bleed.unwrap_or(manifest.codegen.bleed);
        let pattern = dir.join(&input.path);
        let pattern_str = pattern.to_string_lossy().replace('\\', "/");
        for entry in glob::glob(&pattern_str)
            .with_context(|| format!("invalid glob: {}", input.path))?
        {
            let path = entry?;
            if !path.is_file() {
                continue;
            }
            let rel = path.strip_prefix(dir).unwrap_or(&path);
            let rel = rel.to_string_lossy().replace('\\', "/");
            if assets::normalize_path_key_for_input(&rel, input) == key {
                return Ok((path, bleed));
            }
        }
    }
    anyhow::bail!("no manifest file matches lock key {key}");
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
    }
}
