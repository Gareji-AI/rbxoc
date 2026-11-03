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
    // The envelope names the leaf clap matched. `usage` survives only when no
    // subcommand was recognized at all.
    for (args, command) in [
        (vec!["no-such-command"], "usage"),
        (vec!["datastore", "get"], "datastore get"),
        (vec!["--format", "xml", "api", "list"], "api list"),
        (
            vec!["place", "get", "--universe-id", "not-a-number"],
            "place get",
        ),
        (vec!["--format", "text", "no-such-command"], "usage"),
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(64), "{args:?}: {out:?}");
        assert!(out.stderr.is_empty(), "{out:?}");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["command"], command, "{args:?}");
        assert_eq!(json["error"]["code"], "usage_error");
        assert!(!json["error"]["detail"].as_str().unwrap().is_empty());
        // No hint may point at the nonexistent `rbxoc usage` command.
        for hint in json["hints"].as_array().unwrap() {
            assert!(
                !hint.as_str().unwrap().contains("rbxoc usage"),
                "{args:?}: {hint}"
            );
        }
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
fn analytics_catalog_runs_without_credentials() {
    // Fully local: a static list, zero HTTP requests, no authenticated context.
    let out = run(&["analytics", "catalog"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "analytics catalog");
    assert!(
        !json["data"]["metrics"].as_array().unwrap().is_empty(),
        "{json}"
    );
    // The cloud-backed analytics commands still require credentials.
    for args in [
        vec![
            "analytics",
            "metrics",
            "--universe-id",
            "1",
            "--query",
            r#"{"metric":"DailyActiveUsers","granularity":"OneDay","startTime":"2026-10-01T00:00:00Z","endTime":"2026-10-08T00:00:00Z"}"#,
        ],
        vec![
            "analytics",
            "operation",
            "--universe-id",
            "1",
            "--kind",
            "metrics",
            "--operation-id",
            "m1",
        ],
        vec!["analytics", "alerts", "list", "--universe-id", "1"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["error"]["code"], "auth_missing", "{args:?}");
    }
}

#[test]
fn api_key_only_endpoints_reject_an_oauth_only_credential_locally() {
    // DataStores document roblox-api-key only, so an OAuth token must fail before
    // any request is built instead of sending an undocumented header.
    for args in [
        vec![
            "datastore",
            "get",
            "--universe-id",
            "1",
            "--store",
            "s",
            "--entry",
            "e",
        ],
        vec![
            "thumbnail",
            "list",
            "--universe-id",
            "1",
            "--next-cursor",
            "c",
        ],
        vec!["place", "history", "--place-id", "1", "--limit", "5"],
        vec![
            "permissions",
            "grant",
            "--asset-id",
            "111",
            "--subject-type",
            "universe",
            "--subject-id",
            "222",
            "--action",
            "copy-from-rcc",
        ],
    ] {
        let out = run_in(
            &tempfile::tempdir().unwrap().keep(),
            &args,
            &[("RBXOC_OAUTH_TOKEN", "fixture-oauth-token")],
        );
        assert_eq!(out.status.code(), Some(1), "{args:?}: {out:?}");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["ok"], false, "{args:?}");
        assert_eq!(json["error"]["code"], "auth_kind_unsupported", "{args:?}");
        let detail = json["error"]["detail"].as_str().unwrap_or_default();
        assert!(detail.contains("oauth"), "{args:?}: {detail}");
        assert!(detail.contains("api-key"), "{args:?}: {detail}");
        assert!(
            !json["hints"].as_array().unwrap().iter().any(|hint| hint
                .as_str()
                .unwrap()
                .contains("need `RBXOC_OAUTH_TOKEN` instead")),
            "{args:?}: {json}"
        );
    }
}

#[test]
fn dual_auth_endpoints_still_accept_either_credential_family() {
    // No request can reach Roblox in a test, so a dual-auth command must get
    // past the auth-kind preflight and fail on the network instead.
    let dir = tempfile::tempdir().unwrap();
    let args = ["universe", "get", "--universe-id", "1"];
    let oauth = run_in(
        dir.path(),
        &args,
        &[("RBXOC_OAUTH_TOKEN", "fixture-oauth-token")],
    );
    let json: serde_json::Value = serde_json::from_slice(&oauth.stdout).unwrap();
    assert_ne!(
        json["error"]["code"], "auth_kind_unsupported",
        "a dual-auth endpoint must accept OAuth: {json}"
    );
    let key = run(&[
        "--api-key",
        "fixture-not-a-real-key",
        "universe",
        "get",
        "--universe-id",
        "1",
    ]);
    let json: serde_json::Value = serde_json::from_slice(&key.stdout).unwrap();
    assert_ne!(json["error"]["code"], "auth_kind_unsupported", "{json}");
}

#[test]
fn usage_errors_name_the_invoked_leaf_and_never_suggest_rbxoc_usage() {
    for (args, expected) in [
        (
            vec!["datastore", "set", "--universe-id", "1"],
            "datastore set",
        ),
        (
            vec![
                "place",
                "update",
                "--universe-id",
                "1",
                "--place-id",
                "2",
                "--server-size",
                "not-a-number",
            ],
            "place update",
        ),
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(64), "{args:?}");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["command"], expected, "{args:?}");
        let hints: Vec<&str> = json["hints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hint| hint.as_str().unwrap())
            .collect();
        assert!(
            !hints.iter().any(|hint| hint.contains("rbxoc usage")),
            "{args:?}: {hints:?}"
        );
        assert!(
            hints
                .iter()
                .any(|hint| hint.contains(&format!("rbxoc {expected} --help"))),
            "{args:?}: {hints:?}"
        );
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

fn run_in(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rbxoc"));
    cmd.args(args)
        .current_dir(dir)
        .env_remove("RBXOC_API_KEY")
        .env_remove("RBXOC_OAUTH_TOKEN")
        .env_remove("RBXOC_FORMAT");
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.output().unwrap()
}

fn assert_secret_absent(out: &Output, secret: &str) {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stdout.contains(secret), "stdout leaked a credential");
    assert!(!stderr.contains(secret), "stderr leaked a credential");
}

#[test]
fn dotenv_supplies_the_api_key_and_a_preset_env_wins() {
    let dir = tempfile::tempdir().unwrap();
    let file_secret = "abcd-from-dotenv";
    std::fs::write(
        dir.path().join(".env"),
        format!("RBXOC_API_KEY={file_secret}\n"),
    )
    .unwrap();

    let from_file = run_in(dir.path(), &["auth", "check"], &[]);
    assert_secret_absent(&from_file, file_secret);
    assert!(from_file.status.success(), "{}", from_file.status);
    let json: serde_json::Value = serde_json::from_slice(&from_file.stdout).unwrap();
    let source = json["data"]["source"].as_str().unwrap();
    assert!(source.ends_with("(RBXOC_API_KEY)"), "{source}");
    assert!(source.contains(dir.path().join(".env").display().to_string().as_str()));
    assert!(json["summary"].as_str().unwrap().contains("abcd****"));

    let env_secret = "envv-wins-over-file";
    let from_env = run_in(
        dir.path(),
        &["auth", "check"],
        &[("RBXOC_API_KEY", env_secret)],
    );
    assert_secret_absent(&from_env, env_secret);
    assert_secret_absent(&from_env, file_secret);
    assert!(from_env.status.success(), "{}", from_env.status);
    let json: serde_json::Value = serde_json::from_slice(&from_env.stdout).unwrap();
    assert_eq!(json["data"]["source"], "RBXOC_API_KEY");
    assert!(json["summary"].as_str().unwrap().contains("envv****"));
}

#[test]
fn nested_dotenv_does_not_inherit_the_parent_api_key() {
    let root = tempfile::tempdir().unwrap();
    let parent_secret = "parent-key-value";
    std::fs::write(
        root.path().join(".env"),
        format!("RBXOC_API_KEY={parent_secret}\n"),
    )
    .unwrap();
    let child = root.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(child.join(".env"), "OTHER=1\n").unwrap();

    let out = run_in(&child, &["auth", "check"], &[]);
    assert_secret_absent(&out, parent_secret);
    assert_eq!(out.status.code(), Some(2), "{}", out.status);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "auth_missing");
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
fn readme_install_pins_match_the_crate_version() {
    // A stale pin installs a binary without the commands the README documents.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let version = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.split('"').next())
        .expect("Cargo.toml must declare a package version");
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    let pins: Vec<&str> = readme
        .lines()
        .filter_map(|line| line.trim().strip_prefix("rbxoc = \"Gareji-AI/rbxoc@"))
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert!(
        !pins.is_empty(),
        "the README must document at least one Rokit install pin"
    );
    let unique: std::collections::BTreeSet<&str> = pins.iter().copied().collect();
    assert_eq!(
        unique.len(),
        1,
        "duplicate install sections must not pin different releases: {pins:?}"
    );
    assert_eq!(unique.first().copied(), Some(version), "stale install pin");
    // The archive and checksum examples must name the same release.
    assert!(
        readme.contains(&format!("`{version}` carries")),
        "{version}"
    );
    assert!(readme.contains(&format!("rbxoc-{version}-")), "{version}");
}

#[test]
fn readme_event_example_satisfies_the_create_contract() {
    // The copy-paste example must be a document the service would accept, and
    // the incomplete one from earlier releases must not come back.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    let document = readme
        .lines()
        .skip_while(|line| !line.contains("rbxoc event create"))
        .find_map(|line| line.split("--document '").nth(1))
        .and_then(|rest| rest.split('\'').next())
        .expect("the README must show an `event create --document` example");
    let body: serde_json::Value = serde_json::from_str(document).unwrap();
    for field in ["title", "subtitle", "startTime", "endTime", "visibility"] {
        assert!(
            body.get(field).is_some(),
            "{field} is missing from {document}"
        );
    }
    assert!(matches!(
        body["visibility"].as_str(),
        Some("public" | "private")
    ));
    assert!(!readme.contains("--document '{\"title\":\"Winter\",\"startTime\""));
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
