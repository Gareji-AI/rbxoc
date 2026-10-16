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
