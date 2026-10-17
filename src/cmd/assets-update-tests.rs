//! Synthetic Model receipts and local HTTP fixtures. No live asset guarantees.
use super::*;

const KEY: &str = "assets/model.fbx";
const ID: u64 = 501;

fn changed_model() -> tempfile::TempDir {
    let dir = fixture();
    std::fs::write(dir.path().join(KEY), b"fixture model version one").unwrap();
    receipt(dir.path(), KEY, ID, "Model");
    std::fs::write(dir.path().join(KEY), b"fixture model version two").unwrap();
    dir
}

fn receipt(dir: &Path, key: &str, id: u64, asset_type: &str) {
    let manifest = assets::load_manifest(dir).unwrap();
    let sources = assets::collect_sources(dir, &manifest).unwrap();
    let mut lock = assets::load_lockfile(dir).unwrap();
    lock.assets.insert(key.into(), id);
    lock.publications.insert(
        key.into(),
        assets::PublicationReceipt {
            asset_id: id,
            fingerprint: assets::fingerprint(&sources[key]).unwrap(),
            creator: manifest.creator,
            asset_type: asset_type.into(),
        },
    );
    assets::save_lockfile(dir, &lock).unwrap();
}

fn selection() -> Vec<String> {
    vec![KEY.into()]
}

fn updates(server: &MockServer) -> usize {
    server
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.starts_with("PATCH "))
        .count()
}

fn success(request: &str, body: &str, _: usize) -> (u16, serde_json::Value) {
    if request.starts_with("PATCH ") {
        assert!(
            request.starts_with("PATCH /assets/v1/assets/501 "),
            "{request}"
        );
        assert!(body.contains("\"assetId\":\"501\""));
        assert!(!body.contains("assetType"));
        assert!(!body.contains("displayName"));
        assert!(body.contains("fixture model version two"));
        (200, json!({"path": "operations/model-update"}))
    } else {
        assert!(
            request.starts_with("GET /assets/v1/operations/model-update "),
            "{request}"
        );
        (200, json!({"done": true, "response": {"assetId": "501"}}))
    }
}

#[tokio::test]
async fn changed_model_updates_in_place_and_records_current_freshness() {
    let dir = changed_model();
    let original = assets::load_lockfile(dir.path()).unwrap().publications[KEY]
        .fingerprint
        .clone();
    let server = MockServer::new(success);
    let out = run_update(&server.ctx(), dir.path(), &selection()).await;
    assert!(out.ok, "{out:?}");
    let data = out.data.unwrap();
    assert_eq!(data["updated"], 1);
    assert_eq!(data["uploaded"], 0);
    assert_eq!(data["changed"], json!([]));
    assert_eq!(data["current"], json!([KEY]));
    assert_eq!(server.creates(), 0);
    assert_eq!(updates(&server), 1);
    let lock = assets::load_lockfile(dir.path()).unwrap();
    assert_eq!(lock.assets[KEY], ID);
    assert_eq!(lock.publications[KEY].asset_id, ID);
    assert_ne!(lock.publications[KEY].fingerprint, original);
    assert!(lock.pending.is_empty());
    let manifest = assets::load_manifest(dir.path()).unwrap();
    let sources = assets::collect_sources(dir.path(), &manifest).unwrap();
    assert_eq!(
        assets::freshness(&sources, &selection(), &manifest, &lock)
            .unwrap()
            .current,
        selection()
    );
    assert!(run_dry_run(dir.path(), &selection()).ok);
    let generated = std::fs::read_to_string(dir.path().join("generated/assets.luau")).unwrap();
    assert!(generated.contains("rbxassetid://501"));
}

#[tokio::test]
async fn dry_run_lists_updates_without_receipt_or_output_writes() {
    let dir = changed_model();
    let before = std::fs::read(dir.path().join(assets::LOCKFILE_NAME)).unwrap();
    let out = preview_update(dir.path(), &selection()).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(
        out.data.unwrap()["updates"],
        json!([{"path": KEY, "assetId": ID, "action": "update", "assetType": "Model"}])
    );
    assert_eq!(
        std::fs::read(dir.path().join(assets::LOCKFILE_NAME)).unwrap(),
        before
    );
    assert!(!dir.path().join("generated").exists());
    assert!(!dir.path().join(assets::MUTATION_LOCK_NAME).exists());
}

#[tokio::test]
async fn no_path_missing_unverified_and_current_are_all_local_failures() {
    let dir = changed_model();
    let server = MockServer::new(success);
    assert_eq!(
        run_update(&server.ctx(), dir.path(), &[])
            .await
            .error
            .unwrap()
            .code,
        "update_selection_required"
    );
    assert!(!preview_update(dir.path(), &[]).await.ok);
    for state in ["current", "unverified", "missing"] {
        let mut lock = assets::load_lockfile(dir.path()).unwrap();
        match state {
            "current" => {
                let manifest = assets::load_manifest(dir.path()).unwrap();
                let sources = assets::collect_sources(dir.path(), &manifest).unwrap();
                lock.publications.get_mut(KEY).unwrap().fingerprint =
                    assets::fingerprint(&sources[KEY]).unwrap();
            }
            "unverified" => {
                lock.publications.remove(KEY);
            }
            _ => {
                lock.assets.remove(KEY);
            }
        }
        assets::save_lockfile(dir.path(), &lock).unwrap();
        let out = run_update(&server.ctx(), dir.path(), &selection()).await;
        assert_eq!(
            out.error.unwrap().code,
            "asset_update_not_changed",
            "{state}"
        );
        assert!(!preview_update(dir.path(), &selection()).await.ok);
    }
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unsupported_later_file_blocks_entire_update_batch() {
    let dir = changed_model();
    png(dir.path(), "z.png");
    receipt(dir.path(), "assets/z.png", 502, "Decal");
    image::RgbaImage::new(3, 3)
        .save(dir.path().join("assets/z.png"))
        .unwrap();
    let server = MockServer::new(success);
    let keys = vec![KEY.into(), "assets/z.png".into()];
    assert_eq!(
        run_update(&server.ctx(), dir.path(), &keys)
            .await
            .error
            .unwrap()
            .code,
        "asset_update_unsupported"
    );
    assert!(!preview_update(dir.path(), &keys).await.ok);
    assert!(server.requests.lock().unwrap().is_empty());
    assert!(
        assets::load_lockfile(dir.path())
            .unwrap()
            .pending
            .is_empty()
    );
}

#[tokio::test]
async fn interrupted_update_resumes_the_operation_without_another_patch() {
    let dir = changed_model();
    let failed = Arc::new(AtomicBool::new(false));
    let injected = failed.clone();
    let root = dir.path().to_path_buf();
    let server = MockServer::new(move |request, body, n| {
        if request.starts_with("PATCH ") {
            let lock = assets::load_lockfile(&root).unwrap();
            assert_eq!(lock.pending[KEY].target_asset_id, Some(ID));
            assert!(lock.pending[KEY].operation_id.is_none());
        } else if !injected.swap(true, Ordering::Relaxed) {
            return (503, json!({"message": "fixture polling interruption"}));
        }
        success(request, body, n)
    });
    assert!(!run_update(&server.ctx(), dir.path(), &selection()).await.ok);
    let lock = assets::load_lockfile(dir.path()).unwrap();
    assert_eq!(lock.assets[KEY], ID);
    assert_eq!(
        lock.pending[KEY].operation_id.as_deref(),
        Some("model-update")
    );
    assert_ne!(
        lock.publications[KEY].fingerprint,
        lock.pending[KEY].fingerprint
    );
    assert_eq!(
        preview_update(dir.path(), &selection()).await.data.unwrap()["updates"][0]["action"],
        "resume-update"
    );
    let out = run_update(&server.ctx(), dir.path(), &selection()).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.data.unwrap()["resumed"], 1);
    assert_eq!(updates(&server), 1);
    assert_eq!(server.creates(), 0);
    assert!(run_dry_run(dir.path(), &selection()).ok);
}

#[tokio::test]
async fn unknown_update_outcome_is_never_reissued_even_with_retry_enabled() {
    for response in [
        (200, json!({})),
        (503, json!({"message": "uncertain update"})),
    ] {
        let dir = changed_model();
        let server = MockServer::new(move |request, body, n| {
            if request.starts_with("PATCH ") {
                return response.clone();
            }
            success(request, body, n)
        });
        let mut ctx = server.ctx();
        ctx.retry = 3;
        let mut original_clue = None;
        for _ in 0..2 {
            let out = run_update(&ctx, dir.path(), &selection()).await;
            assert_eq!(out.error.as_ref().unwrap().code, "publication_uncertain");
            let clue = out.data.as_ref().unwrap()["pendingPublications"][0].clone();
            assert_eq!(clue["path"], KEY);
            assert_eq!(clue["targetAssetId"], ID);
            assert_eq!(clue["assetType"], "Model");
            assert!(clue["displayName"].is_null()); // Content updates never send a new name.
            assert!(clue["requestedAt"].as_str().is_some());
            if let Some(original) = &original_clue {
                assert_eq!(&clue, original);
            }
            original_clue = Some(clue);
        }
        assert_eq!(updates(&server), 1);
        let lock = assets::load_lockfile(dir.path()).unwrap();
        assert_eq!(lock.assets[KEY], ID);
        assert!(lock.pending[KEY].operation_id.is_none());
        assert!(recover_operation(dir.path(), KEY, "model-update").ok);
        assert!(run_update(&ctx, dir.path(), &selection()).await.ok);
        assert_eq!(updates(&server), 1);
    }
}

#[tokio::test]
async fn changed_recovery_source_creator_or_target_cannot_be_rebound() {
    for changed in ["source", "creator", "target"] {
        let dir = changed_model();
        let server = MockServer::new(|_, _, _| (200, json!({})));
        assert!(!run_update(&server.ctx(), dir.path(), &selection()).await.ok);
        match changed {
            "source" => std::fs::write(dir.path().join(KEY), b"different version").unwrap(),
            "creator" => {
                let mut manifest = assets::load_manifest(dir.path()).unwrap();
                manifest.creator.id = 8;
                write_manifest(dir.path(), &manifest);
            }
            _ => {
                let mut lock = assets::load_lockfile(dir.path()).unwrap();
                lock.assets.insert(KEY.into(), 777);
                assets::save_lockfile(dir.path(), &lock).unwrap();
            }
        }
        assert!(
            !recover_operation(dir.path(), KEY, "model-update").ok,
            "{changed}"
        );
        assert!(
            !run_update(&server.ctx(), dir.path(), &selection()).await.ok,
            "{changed}"
        );
        assert_eq!(updates(&server), 1);
    }
}

#[tokio::test]
async fn mismatched_completion_id_preserves_original_id_and_pending_receipt() {
    let dir = changed_model();
    let server = MockServer::new(|request, _, _| {
        if request.starts_with("PATCH ") {
            (200, json!({"path": "operations/model-update"}))
        } else {
            (200, json!({"done": true, "response": {"assetId": "999"}}))
        }
    });
    for _ in 0..2 {
        assert_eq!(
            run_update(&server.ctx(), dir.path(), &selection())
                .await
                .error
                .unwrap()
                .code,
            "asset_id_mismatch"
        );
    }
    let lock = assets::load_lockfile(dir.path()).unwrap();
    assert_eq!(lock.assets[KEY], ID);
    assert_eq!(lock.pending[KEY].target_asset_id, Some(ID));
    assert_eq!(updates(&server), 1);
    assert!(!run_dry_run(dir.path(), &selection()).ok);
}

#[tokio::test]
async fn only_selected_changed_models_update_and_normal_sync_cannot_resume_them() {
    let dir = changed_model();
    std::fs::write(dir.path().join("assets/missing.fbx"), b"missing fixture").unwrap();
    std::fs::write(dir.path().join("assets/other.fbx"), b"other fixture").unwrap();
    receipt(dir.path(), "assets/other.fbx", 502, "Model");
    std::fs::write(
        dir.path().join("assets/other.fbx"),
        b"changed other fixture",
    )
    .unwrap();
    let server = MockServer::new(|_, _, _| (200, json!({})));
    assert!(!run_update(&server.ctx(), dir.path(), &selection()).await.ok);
    let before = server.requests.lock().unwrap().len();
    assert!(!run(&server.ctx(), dir.path(), &selection()).await.ok);
    assert_eq!(server.requests.lock().unwrap().len(), before);
    let lock = assets::load_lockfile(dir.path()).unwrap();
    assert!(!lock.assets.contains_key("assets/missing.fbx"));
    assert!(!lock.pending.contains_key("assets/other.fbx"));
    assert_eq!(lock.assets["assets/other.fbx"], 502);
    assert_eq!(updates(&server), 1);
    assert_eq!(server.creates(), 0);
}

#[tokio::test]
async fn duplicate_update_targets_and_unsafe_output_are_rejected_locally() {
    let dir = changed_model();
    std::fs::write(dir.path().join("assets/alias.fbx"), b"alias fixture").unwrap();
    receipt(dir.path(), "assets/alias.fbx", ID, "Model");
    std::fs::write(
        dir.path().join("assets/alias.fbx"),
        b"changed alias fixture",
    )
    .unwrap();
    let server = MockServer::new(success);
    assert_eq!(
        run_update(
            &server.ctx(),
            dir.path(),
            &[KEY.into(), "assets/alias.fbx".into()]
        )
        .await
        .error
        .unwrap()
        .code,
        "update_target_collision"
    );
    let mut manifest = assets::load_manifest(dir.path()).unwrap();
    manifest.codegen.output_path = "assets".into();
    manifest.codegen.output_basename = Some("model.fbx".into());
    write_manifest(dir.path(), &manifest);
    assert!(!run_update(&server.ctx(), dir.path(), &selection()).await.ok);
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn model_receipt_cannot_authorize_new_animation_content() {
    let dir = fixture();
    let key = "assets/model.rbxmx";
    std::fs::write(
        dir.path().join(key),
        r#"<roblox version="4"><Item class="Model" referent="RBX0"><Properties/></Item></roblox>"#,
    )
    .unwrap();
    receipt(dir.path(), key, ID, "Model");
    std::fs::write(dir.path().join(key), r#"<roblox version="4"><Item class="KeyframeSequence" referent="RBX0"><Properties/></Item></roblox>"#).unwrap();
    let server = MockServer::new(success);
    let keys = vec![key.into()];
    assert_eq!(
        run_update(&server.ctx(), dir.path(), &keys)
            .await
            .error
            .unwrap()
            .code,
        "asset_update_unsupported"
    );
    assert!(!preview_update(dir.path(), &keys).await.ok);
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn explicit_update_rejection_leaves_old_receipt_and_allows_a_new_attempt() {
    let dir = changed_model();
    let before = assets::load_lockfile(dir.path()).unwrap().publications[KEY]
        .fingerprint
        .clone();
    let failed = Arc::new(AtomicBool::new(false));
    let injected = failed.clone();
    let server = MockServer::new(move |request, body, n| {
        if request.starts_with("PATCH ") && !injected.swap(true, Ordering::Relaxed) {
            return (403, json!({"message": "explicit rejection"}));
        }
        success(request, body, n)
    });
    assert!(!run_update(&server.ctx(), dir.path(), &selection()).await.ok);
    let lock = assets::load_lockfile(dir.path()).unwrap();
    assert!(lock.pending.is_empty());
    assert_eq!(lock.publications[KEY].fingerprint, before);
    assert!(run_update(&server.ctx(), dir.path(), &selection()).await.ok);
    assert_eq!(updates(&server), 2);
}

#[tokio::test]
async fn partial_batch_commits_each_update_and_resumes_only_unfinished_paths() {
    let dir = changed_model();
    let other = "assets/other.fbx";
    std::fs::write(dir.path().join(other), b"old other fixture").unwrap();
    receipt(dir.path(), other, 502, "Model");
    std::fs::write(dir.path().join(other), b"new other fixture").unwrap();
    let failed = Arc::new(AtomicBool::new(false));
    let injected = failed.clone();
    let root = dir.path().to_path_buf();
    let server = MockServer::new(move |request, _, _| {
        if request.starts_with("PATCH /assets/v1/assets/501 ") {
            (200, json!({"path": "operations/first"}))
        } else if request.starts_with("PATCH /assets/v1/assets/502 ") {
            let lock = assets::load_lockfile(&root).unwrap();
            assert!(!lock.pending.contains_key(KEY));
            assert_eq!(
                lock.publications[KEY].fingerprint,
                assets::fingerprint(&assets::Source {
                    path: root.join(KEY),
                    bleed: true
                })
                .unwrap()
            );
            (200, json!({"path": "operations/second"}))
        } else if request.starts_with("GET /assets/v1/operations/first ") {
            (200, json!({"done": true, "response": {"assetId": "501"}}))
        } else if !injected.swap(true, Ordering::Relaxed) {
            (503, json!({"message": "second poll interrupted"}))
        } else {
            (200, json!({"done": true, "response": {"assetId": "502"}}))
        }
    });
    let both = vec![KEY.into(), other.into()];
    assert!(!run_update(&server.ctx(), dir.path(), &both).await.ok);
    let lock = assets::load_lockfile(dir.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("generated/assets.luau")).unwrap(),
        assets::render(&assets::load_manifest(dir.path()).unwrap(), &lock).unwrap()
    );
    let count = server.requests.lock().unwrap().len();
    assert_eq!(
        run_update(&server.ctx(), dir.path(), &both)
            .await
            .error
            .unwrap()
            .code,
        "asset_update_not_changed"
    );
    assert_eq!(server.requests.lock().unwrap().len(), count);
    let out = run_update(&server.ctx(), dir.path(), &[other.into()]).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.data.unwrap()["resumed"], 1);
    assert_eq!(updates(&server), 2);
    assert_eq!(server.creates(), 0);
    assert!(run_dry_run(dir.path(), &both).ok);
}
