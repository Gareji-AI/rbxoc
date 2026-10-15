//! Upload missing manifest files, refresh the lockfile, and run codegen.

use std::path::Path;

use anyhow::Context;
use serde_json::json;

use crate::cmd::{asset, assets, preprocess};
use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

const COMMAND: &str = "assets sync";

/// Freshness check only. No API credentials required.
pub fn run_dry_run(dir: &Path, selection: &[String]) -> Output {
    dry_run_report(dir, selection)
}

/// Upload missing files, update the lockfile, and write generated source.
pub async fn run(ctx: &Ctx, dir: &Path, selection: &[String]) -> Output {
    let _guard = match assets::registry_guard(dir) {
        Ok(guard) => guard,
        Err(e) => return registry_failure(e),
    };
    let (manifest, mut lock) = match load_registry(dir) {
        Ok(v) => v,
        Err(out) => return *out,
    };
    let paths = match assets::collect_paths(dir, &manifest) {
        Ok(p) => p,
        Err(e) => return registry_failure(e),
    };
    let missing = assets::missing_ids(&paths, &lock);

    let selected = match assets::select_paths(&paths, selection) {
        Ok(selected) => selected,
        Err(e) => return registry_failure(e),
    };
    let missing: Vec<_> = missing
        .into_iter()
        .filter(|key| selected.contains(key))
        .collect();
    let mut prospective: std::collections::BTreeSet<_> =
        lock.assets.keys().map(String::as_str).collect();
    prospective.extend(missing.iter().map(String::as_str));
    if let Err(e) =
        crate::cmd::codegen::validate_keys(prospective, &assets::codegen_options(&manifest.codegen))
    {
        return registry_failure(e);
    }

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

    if uploaded > 0
        && let Err(e) = assets::save_lockfile(dir, &lock)
    {
        return registry_failure(e);
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
            "selected": selected,
        })),
    )
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
    });
    if missing.is_empty() && freshness.changed.is_empty() && pending.is_empty() {
        return Output::success(
            COMMAND,
            format!("All {} selected files have asset IDs ({} unverified)", paths.len(), freshness.unverified.len()),
            Some(data),
        ).with_hints(vec!["Registry check only: unverified entries are not proof of freshness; no preprocessing or upload preflight was performed.".into()]);
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
        for entry in
            glob::glob(&pattern_str).with_context(|| format!("invalid glob: {}", input.path))?
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
