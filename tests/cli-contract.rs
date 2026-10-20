//! Credential-free process-level contracts, including clap's early exit paths.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_rbxoc"))
        .args(args)
        .current_dir(dir.path())
        .env_remove("RBXOC_API_KEY")
        .env_remove("RBXOC_OAUTH_TOKEN")
        .env_remove("RBXOC_FORMAT")
        .output()
        .unwrap()
}

#[test]
fn usage_errors_are_json_on_stdout_and_exit_64() {
    for args in [
        vec!["no-such-command"],
        vec!["datastore", "get"],
        vec!["--format", "xml", "api", "list"],
        vec!["place", "get", "--universe-id", "not-a-number"],
        vec!["--format", "text", "no-such-command"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(64), "{args:?}: {out:?}");
        assert!(out.stderr.is_empty(), "{out:?}");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["command"], "usage");
        assert_eq!(json["error"]["code"], "usage_error");
        assert!(!json["error"]["detail"].as_str().unwrap().is_empty());
    }
}

#[test]
fn documented_exit_codes_do_not_collide() {
    for (args, code, error) in [
        (vec!["auth", "check"], 2, "auth_missing"),
        (
            vec!["api", "show", "no.such.endpoint"],
            64,
            "unknown_endpoint",
        ),
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(code));
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["error"]["code"], error);
    }
}

#[test]
fn local_validation_registry_and_file_failures_have_nonempty_hints() {
    for args in [
        vec![
            "--api-key",
            "fixture-not-a-real-key",
            "analytics",
            "metrics",
            "--universe-id",
            "1",
            "--query",
            "nope",
        ],
        vec!["assets", "status"],
        vec![
            "--api-key",
            "fixture-not-a-real-key",
            "thumbnail",
            "upload",
            "--universe-id",
            "1",
            "--files",
            "missing-fixture.png",
        ],
        vec!["api", "list", "--auth", "invalid-fixture"],
    ] {
        let out = run(&args);
        assert!(!out.status.success(), "{out:?}");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["ok"], false);
        let hints = json["hints"]
            .as_array()
            .expect("every failure must include hints");
        assert!(!hints.is_empty(), "{json}");
        let mut text_args = vec!["--format", "text"];
        text_args.extend(&args);
        let text = String::from_utf8(run(&text_args).stdout).unwrap();
        for hint in hints {
            let hint = hint.as_str().unwrap();
            assert!(!hint.trim().is_empty());
            assert!(text.contains(hint), "{text}");
        }
    }
}

#[test]
fn metadata_only_asset_update_is_credential_free_in_dry_run() {
    let out = run(&[
        "asset",
        "update",
        "--asset-id",
        "123",
        "--display-name",
        "Renamed",
        "--description",
        "New description",
        "--icon",
        "assets/456",
        "--preview",
        "assets/789",
        "--dry-run",
    ]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["data"]["updateMask"],
        "displayName,description,icon,previews"
    );
    assert_eq!(json["data"]["request"]["icon"], "assets/456");
    assert_eq!(
        json["data"]["request"]["previews"],
        serde_json::json!([{"asset": "assets/789"}])
    );
    assert_eq!(json["data"]["bytes"], 0);
}

#[test]
fn invalid_asset_metadata_fails_before_authentication() {
    for (args, code) in [
        (
            vec!["asset", "update", "--asset-id", "123"],
            "nothing_to_update",
        ),
        (
            vec!["asset", "update", "--asset-id", "123", "--icon", "icon.png"],
            "invalid_image_resource",
        ),
    ] {
        let out = run(&args);
        assert!(!out.status.success());
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["error"]["code"], code);
    }
}

#[test]
fn reviewer_asset_update_preflight_preserves_the_requested_command_label() {
    for dry_run in [false, true] {
        let mut args = vec!["asset", "update", "--asset-id", "123"];
        if dry_run {
            args.push("--dry-run");
        }
        let out = run(&args);
        assert!(!out.status.success());
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["error"]["code"], "nothing_to_update");
        assert_eq!(
            json["command"],
            if dry_run {
                "asset update (dry-run)"
            } else {
                "asset update"
            }
        );
        if !dry_run {
            assert!(
                json["hints"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|hint| !hint.as_str().unwrap().contains("(dry-run)"))
            );
            assert!(
                String::from_utf8(out.stdout)
                    .unwrap()
                    .contains("rbxoc asset update --help")
            );
        }
    }
}

#[test]
fn help_and_version_remain_successful_human_readable_output() {
    for arg in ["--help", "--version"] {
        let out = run(&[arg]);
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        assert!(String::from_utf8(out.stdout).unwrap().contains("rbxoc"));
    }
}

#[test]
fn auth_check_handles_unicode_and_reports_the_exact_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("key.txt");
    std::fs::write(&path, "日本語abcdefgh").unwrap();
    let out = run(&["--api-key-file", path.to_str().unwrap(), "auth", "check"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["data"]["source"], path.display().to_string());
    assert!(json["summary"].as_str().unwrap().contains("日本語a****"));
    assert!(!String::from_utf8(out.stdout).unwrap().contains("abcdefgh"));
}

#[test]
fn update_requires_explicit_paths_before_any_authentication() {
    let out = run(&["assets", "sync", "--update"]);
    assert_eq!(out.status.code(), Some(64));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "usage_error");
}

#[test]
fn update_eligibility_and_dry_run_are_credential_free() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join("assets/model.fbx"),
        b"synthetic changed Model fixture",
    )
    .unwrap();
    std::fs::write(dir.path().join("rbxoc.assets.toml"), "[creator]\ntype = 'group'\nid = 7\n[codegen]\noutput_path = 'generated'\n[inputs.main]\npath = 'assets/*'\n").unwrap();
    let args = [
        "assets",
        "sync",
        "--dir",
        dir.path().to_str().unwrap(),
        "--update",
        "--path",
        "assets/model.fbx",
    ];
    let missing = run(&args);
    let json: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(json["error"]["code"], "asset_update_not_changed");
    // A synthetic historical receipt, not a claim about any real asset.
    let lock = "version = 1\n[assets]\n'assets/model.fbx' = 501\n[publications.'assets/model.fbx']\nasset_id = 501\nfingerprint = 'previous-published-fixture'\nasset_type = 'Model'\n[publications.'assets/model.fbx'.creator]\ntype = 'group'\nid = 7\n";
    std::fs::write(dir.path().join("rbxoc.assets.lock.toml"), lock).unwrap();
    let mut preview_args = args.to_vec();
    preview_args.push("--dry-run");
    let out = run(&preview_args);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["data"]["updates"][0]["assetId"], 501);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("rbxoc.assets.lock.toml")).unwrap(),
        lock
    );
    assert!(!dir.path().join("generated").exists());
}

#[test]
fn credential_and_document_failures_keep_the_leaf_command() {
    let missing = run(&[
        "datastore",
        "get",
        "--universe-id",
        "1",
        "--store",
        "test",
        "--entry",
        "player",
    ]);
    assert_eq!(missing.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(json["command"], "datastore get");
    assert_eq!(json["error"]["code"], "auth_missing");
    assert!(json["hints"].as_array().unwrap().iter().any(|hint| {
        hint.as_str()
            .unwrap()
            .contains("rbxoc datastore get --help")
    }));
    let text = String::from_utf8(
        run(&[
            "--format",
            "text",
            "datastore",
            "get",
            "--universe-id",
            "1",
            "--store",
            "test",
            "--entry",
            "player",
        ])
        .stdout,
    )
    .unwrap();
    assert!(text.contains("datastore get"));
    assert!(text.contains("rbxoc datastore get --help"));

    let auth = run(&["auth", "check"]);
    let json: serde_json::Value = serde_json::from_slice(&auth.stdout).unwrap();
    assert_eq!(json["command"], "auth check");

    for (args, command) in [
        (
            vec![
                "--api-key",
                "fixture-not-a-real-key",
                "config",
                "patch-draft",
                "--universe-id",
                "1",
                "--repository",
                "InExperienceConfig",
                "--document",
                "{",
            ],
            "config patch-draft",
        ),
        (
            vec![
                "--api-key",
                "fixture-not-a-real-key",
                "event",
                "create",
                "--universe-id",
                "1",
                "--document",
                "{",
            ],
            "event create",
        ),
        (
            vec![
                "--api-key",
                "fixture-not-a-real-key",
                "analytics",
                "alerts",
                "create",
                "--universe-id",
                "1",
                "--document",
                "{",
            ],
            "analytics alerts create",
        ),
    ] {
        let out = run(&args);
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["command"], command, "{json}");
        assert_eq!(json["error"]["code"], "invalid_document");
        assert!(json["hints"].as_array().unwrap().iter().any(|hint| {
            hint.as_str()
                .unwrap()
                .contains(&format!("rbxoc {command} --help"))
        }));
        let mut text_args = vec!["--format", "text"];
        text_args.extend(args);
        let text = String::from_utf8(run(&text_args).stdout).unwrap();
        assert!(text.contains(command), "{text}");
        assert!(text.contains(&format!("rbxoc {command} --help")), "{text}");
    }
}

#[test]
fn closed_enums_fail_before_authentication() {
    let repository = run(&[
        "config",
        "get",
        "--universe-id",
        "1",
        "--repository",
        "game",
    ]);
    let json: serde_json::Value = serde_json::from_slice(&repository.stdout).unwrap();
    assert_eq!(json["command"], "config get");
    assert_eq!(json["error"]["code"], "invalid_repository");
    assert!(
        json["hints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hint| { hint.as_str().unwrap().contains("rbxoc config get --help") })
    );

    let query = run(&[
        "analytics",
        "metrics",
        "--universe-id",
        "1",
        "--query",
        r#"{"metric":"DailyActiveUsers","granularity":"P1D","startTime":"2026-10-01T00:00:00Z","endTime":"2026-10-08T00:00:00Z"}"#,
    ]);
    let json: serde_json::Value = serde_json::from_slice(&query.stdout).unwrap();
    assert_eq!(json["command"], "analytics metrics");
    assert_eq!(json["error"]["code"], "invalid_query");
}

#[test]
fn asset_size_preflight_is_credential_free() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.png");
    let empty = dir.path().join("empty.fbx");
    std::fs::write(&empty, b"").unwrap();
    let over = dir.path().join("over.png");
    std::fs::File::create(&over)
        .unwrap()
        .set_len(20 * 1024 * 1024 + 1)
        .unwrap();
    let boundary = dir.path().join("boundary.png");
    std::fs::File::create(&boundary)
        .unwrap()
        .set_len(20 * 1024 * 1024)
        .unwrap();

    for (path, code, dry_run) in [
        (missing.as_path(), "file_unreadable", true),
        (empty.as_path(), "file_size_invalid", true),
        (over.as_path(), "file_size_invalid", false),
    ] {
        let mut args = vec![
            "asset",
            "create",
            "--file",
            path.to_str().unwrap(),
            "--display-name",
            "Fixture",
            "--user-id",
            "1",
        ];
        if dry_run {
            args.push("--dry-run");
        }
        let out = run(&args);
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["error"]["code"], code, "{json}");
        assert!(!json["ok"].as_bool().unwrap());
    }

    let out = run(&[
        "asset",
        "create",
        "--file",
        boundary.to_str().unwrap(),
        "--display-name",
        "Fixture",
        "--user-id",
        "1",
        "--dry-run",
    ]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["data"]["bytes"], 20 * 1024 * 1024);
}

#[test]
fn readme_examples_match_the_openapi_excerpt() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let excerpt: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tests/fixtures/openapi-example-contract.json"))
            .unwrap(),
    )
    .unwrap();
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    for snippet in excerpt["rejectedReadmeSnippets"].as_array().unwrap() {
        let snippet = snippet.as_str().unwrap();
        assert!(
            !readme.contains(snippet),
            "{snippet} is still in the README"
        );
    }
    assert!(readme.contains(excerpt["readmeQuery"].as_str().unwrap()));
    assert!(readme.contains(excerpt["readmeDraft"].as_str().unwrap()));
    for name in excerpt["repositories"].as_array().unwrap() {
        assert!(readme.contains(name.as_str().unwrap()));
    }
    for name in excerpt["granularities"].as_array().unwrap() {
        assert!(readme.contains(name.as_str().unwrap()));
    }
}
