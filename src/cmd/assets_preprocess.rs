//! Write preprocessed manifest files to a debug directory (Asphalt `debug` parity).

use std::path::Path;

use serde_json::json;

use crate::cmd::{assets, assets_sync, preprocess};
use crate::ctx::failure;
use crate::output::{ErrorBody, Output};

const COMMAND: &str = "assets preprocess";

pub fn run(dir: &Path, out: &Path) -> Output {
    let out = if out.is_absolute() {
        out.to_path_buf()
    } else {
        dir.join(out)
    };
    let manifest = match assets::load_manifest(dir) {
        Ok(m) => m,
        Err(e) => return registry_failure(e),
    };
    let paths = match assets::collect_paths(dir, &manifest) {
        Ok(p) => p,
        Err(e) => return registry_failure(e),
    };

    if let Err(e) = std::fs::create_dir_all(&out) {
        return failure(
            COMMAND,
            ErrorBody {
                code: "mkdir_failed",
                message: format!("cannot create {}: {e}", out.display()),
                status: None,
                detail: None,
                refs: None,
            },
        );
    }

    let mut written = 0usize;
    for rel in &paths {
        let (file, bleed) = match assets_sync::resolve_file_for_key(dir, &manifest, rel) {
            Ok(v) => v,
            Err(e) => return registry_failure(e),
        };
        let upload = match preprocess::prepare_upload_file(&file, bleed) {
            Ok(u) => u,
            Err(err) => return failure(COMMAND, err),
        };
        let dest = out.join(rel);
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return failure(
                    COMMAND,
                    ErrorBody {
                        code: "mkdir_failed",
                        message: format!("cannot create {}: {e}", parent.display()),
                        status: None,
                        detail: None,
                        refs: None,
                    },
                );
            }
        }
        if let Err(e) = std::fs::copy(upload.as_path(), &dest) {
            return failure(
                COMMAND,
                ErrorBody {
                    code: "copy_failed",
                    message: format!("cannot write {}: {e}", dest.display()),
                    status: None,
                    detail: None,
                    refs: None,
                },
            );
        }
        written += 1;
    }

    Output::success(
        COMMAND,
        format!("Wrote {written} files under {}", out.display()),
        Some(json!({
            "output": out.to_string_lossy(),
            "written": written,
            "total": paths.len(),
        })),
    )
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
