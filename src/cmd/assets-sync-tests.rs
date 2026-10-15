//! Credential-free HTTP fixtures for publication safety. No Roblox calls.

use super::*;
use crate::auth::Auth;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;

struct MockServer {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MockServer {
    fn new(
        handler: impl Fn(&str, &str, usize) -> (u16, serde_json::Value) + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let seen = requests.clone();
        let stop = stopped.clone();
        let thread = std::thread::spawn(move || {
            let mut creates = 0;
            while !stop.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                };
                // Windows may inherit the listener's nonblocking mode.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let header_end = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0, "request ended before headers");
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(index) = bytes.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < header_end + length {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0, "request ended before body");
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let first = headers.lines().next().unwrap();
                if first.starts_with("POST ") {
                    creates += 1;
                }
                let body = String::from_utf8_lossy(&bytes[header_end..]);
                seen.lock().unwrap().push(format!("{first}\n{body}"));
                let (status, data) = handler(first, &body, creates);
                let data = data.to_string();
                write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}", data.len()).unwrap();
            }
        });
        Self {
            url,
            requests,
            stopped,
            thread: Some(thread),
        }
    }

    fn success() -> Self {
        Self::new(success_response)
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap(),
            auth: Auth::ApiKey("fixture-not-a-real-key".into()),
            retry: 0,
            api_host: self.url.clone(),
        }
    }

    fn creates(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.starts_with("POST "))
            .count()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let result = self.thread.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

fn success_response(request: &str, _: &str, creates: usize) -> (u16, serde_json::Value) {
    if request.starts_with("POST ") {
        (200, json!({"path": format!("operations/op{creates}")}))
    } else {
        let operation = request
            .split_whitespace()
            .nth(1)
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap();
        let number: u64 = operation.trim_start_matches("op").parse().unwrap();
        (
            200,
            json!({"done": true, "response": {"assetId": (100 + number).to_string()}}),
        )
    }
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join(assets::MANIFEST_NAME),
        r#"
[creator]
type = "group"
id = 7
[codegen]
output_path = "generated"
[inputs.main]
path = "assets/*"
"#,
    )
    .unwrap();
    dir
}

fn png(dir: &Path, name: &str) {
    image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
        .save(dir.join("assets").join(name))
        .unwrap();
}

fn write_manifest(dir: &Path, manifest: &assets::Manifest) {
    std::fs::write(
        dir.join(assets::MANIFEST_NAME),
        toml::to_string(manifest).unwrap(),
    )
    .unwrap();
}

fn pending(dir: &Path, key: &str, operation: Option<&str>) {
    let manifest = assets::load_manifest(dir).unwrap();
    let sources = assets::collect_sources(dir, &manifest).unwrap();
    let mut lock = assets::load_lockfile(dir).unwrap();
    lock.pending.insert(
        key.into(),
        assets::PendingUpload {
            fingerprint: assets::fingerprint(&sources[key]).unwrap(),
            creator: manifest.creator,
            asset_type: "Decal".into(),
            operation_id: operation.map(str::to_string),
        },
    );
    assets::save_lockfile(dir, &lock).unwrap();
}

#[tokio::test]
async fn valid_first_invalid_later_sends_zero_mutations() {
    let dir = fixture();
    png(dir.path(), "a.png");
    std::fs::write(dir.path().join("assets/z.rbxmx"), b"malformed model").unwrap();
    let server = MockServer::success();
    let out = run(&server.ctx(), dir.path(), &[]).await;
    assert!(!out.ok);
    let error = out.error.unwrap();
    assert_eq!(error.code, "model_parse_failed");
    assert!(error.message.contains("assets/z.rbxmx"));
    assert_eq!(server.creates(), 0);
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn scoped_valid_batch_uploads_only_selected_missing_and_keeps_registry() {
    let dir = fixture();
    png(dir.path(), "a.png");
    png(dir.path(), "registered.png");
    std::fs::write(
        dir.path().join("assets/b.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"/>"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("assets/z.rbxmx"),
        b"invalid unselected model",
    )
    .unwrap();
    let mut lock = assets::Lockfile::new();
    lock.assets.insert("assets/registered.png".into(), 99);
    lock.assets.insert("old.png".into(), 88);
    assets::save_lockfile(dir.path(), &lock).unwrap();
    let before = assets::load_manifest(dir.path()).unwrap().creator;
    let selection = vec![
        "assets/a.png".into(),
        "assets/b.svg".into(),
        "assets/registered.png".into(),
    ];
    let server = MockServer::success();
    let out = run(&server.ctx(), dir.path(), &selection).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(server.creates(), 2);
    let requests = server.requests.lock().unwrap().join("\n");
    assert!(requests.contains("image/png"));
    assert!(requests.contains("\"assetType\":\"Decal\""));
    assert!(!requests.contains("\"displayName\":\"registered\""));
    assert!(!requests.contains("z.rbxmx"));
    let lock = assets::load_lockfile(dir.path()).unwrap();
    assert_eq!(lock.assets["assets/registered.png"], 99);
    assert_eq!(lock.assets["old.png"], 88);
    assert!(!lock.assets.contains_key("assets/z.rbxmx"));
    assert_eq!(lock.publications.len(), 2);
    assert!(lock.pending.is_empty());
    assert_eq!(assets::load_manifest(dir.path()).unwrap().creator, before);
    let generated = std::fs::read_to_string(dir.path().join("generated/assets.luau")).unwrap();
    assert!(generated.contains("rbxassetid://88"));
    assert!(!generated.contains("z.rbxmx"));
    assert!(run_dry_run(dir.path(), &selection).ok);
    assert!(!run_dry_run(dir.path(), &[]).ok);
    assert!(
        !run(
            &server.ctx(),
            dir.path(),
            &["assets/a.png".into(), "unknown.png".into()]
        )
        .await
        .ok
    );
    assert_eq!(server.creates(), 2);
}

#[tokio::test]
async fn partial_completion_is_durable_and_retry_only_polls_pending_operation() {
    let dir = fixture();
    png(dir.path(), "a.png");
    png(dir.path(), "b.png");
    let root = dir.path().to_path_buf();
    let failed = Arc::new(AtomicBool::new(false));
    let injected = failed.clone();
    let server = MockServer::new(move |request, body, creates| {
        if request.starts_with("POST ") && creates == 2 {
            assert_eq!(
                assets::load_lockfile(&root).unwrap().assets["assets/a.png"],
                101
            );
        }
        if request.starts_with("GET /assets/v1/operations/op2 ")
            && !injected.swap(true, Ordering::Relaxed)
        {
            return (503, json!({"message": "injected polling failure"}));
        }
        success_response(request, body, creates)
    });
    assert!(!run(&server.ctx(), dir.path(), &[]).await.ok);
    let lock = assets::load_lockfile(dir.path()).unwrap();
    assert_eq!(lock.assets["assets/a.png"], 101);
    assert_eq!(
        lock.pending["assets/b.png"].operation_id.as_deref(),
        Some("op2")
    );
    assert!(
        !std::fs::read_to_string(dir.path().join(assets::LOCKFILE_NAME))
            .unwrap()
            .contains("fixture-not-a-real-key")
    );
    let retry = run(&server.ctx(), dir.path(), &[]).await;
    assert!(retry.ok, "{retry:?}");
    assert_eq!(retry.data.unwrap()["resumed"], 1);
    assert_eq!(server.creates(), 2);
    let lock = assets::load_lockfile(dir.path()).unwrap();
    assert_eq!(lock.assets["assets/b.png"], 102);
    assert_eq!(lock.publications.len(), 2);
    assert!(lock.pending.is_empty());
}

#[tokio::test]
async fn changed_source_or_creator_refuses_recovery_before_any_request() {
    let dir = fixture();
    png(dir.path(), "a.png");
    pending(dir.path(), "assets/a.png", Some("op1"));
    let original = std::fs::read(dir.path().join("assets/a.png")).unwrap();
    image::RgbaImage::new(3, 3)
        .save(dir.path().join("assets/a.png"))
        .unwrap();
    let server = MockServer::success();
    let changed = run(&server.ctx(), dir.path(), &[]).await;
    assert_eq!(changed.error.unwrap().code, "recovery_input_changed");
    std::fs::write(dir.path().join("assets/a.png"), original).unwrap();
    let mut manifest = assets::load_manifest(dir.path()).unwrap();
    manifest.creator.id = 8;
    write_manifest(dir.path(), &manifest);
    assert_eq!(
        run(&server.ctx(), dir.path(), &[])
            .await
            .error
            .unwrap()
            .code,
        "recovery_input_changed"
    );
    assert!(!recover_operation(dir.path(), "assets/a.png", "op1").ok);
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn missing_create_receipt_stops_retries_until_explicit_recovery() {
    let dir = fixture();
    png(dir.path(), "a.png");
    let server = MockServer::new(|request, body, creates| {
        if request.starts_with("POST ") {
            (200, json!({}))
        } else {
            success_response(request, body, creates)
        }
    });
    let mut ctx = server.ctx();
    ctx.retry = 3;
    assert_eq!(
        run(&ctx, dir.path(), &[]).await.error.unwrap().code,
        "publication_uncertain"
    );
    assert_eq!(
        run(&ctx, dir.path(), &[]).await.error.unwrap().code,
        "publication_uncertain"
    );
    assert_eq!(server.creates(), 1);
    assert!(recover_operation(dir.path(), "assets/a.png", "operations/op1").ok);
    assert!(!recover_operation(dir.path(), "assets/a.png", "op2").ok);
    assert!(run(&ctx, dir.path(), &[]).await.ok);
    assert_eq!(server.creates(), 1);
    assert_eq!(
        assets::load_lockfile(dir.path()).unwrap().assets["assets/a.png"],
        101
    );
}

#[tokio::test]
async fn receipt_write_failure_keeps_recoverable_operation_and_reports_asset_id() {
    let dir = fixture();
    png(dir.path(), "a.png");
    let root = dir.path().to_path_buf();
    let obstructed = Arc::new(AtomicBool::new(false));
    let injected = obstructed.clone();
    let server = MockServer::new(move |request, body, creates| {
        if request.starts_with("GET ") && !injected.swap(true, Ordering::Relaxed) {
            std::fs::rename(
                root.join(assets::LOCKFILE_NAME),
                root.join("saved-receipt.toml"),
            )
            .unwrap();
            std::fs::create_dir(root.join(assets::LOCKFILE_NAME)).unwrap();
        }
        success_response(request, body, creates)
    });
    let out = run(&server.ctx(), dir.path(), &[]).await;
    assert_eq!(out.error.unwrap().code, "receipt_write_failed");
    assert!(out.hints.iter().any(|hint| hint.contains("101")));
    std::fs::remove_dir(dir.path().join(assets::LOCKFILE_NAME)).unwrap();
    std::fs::rename(
        dir.path().join("saved-receipt.toml"),
        dir.path().join(assets::LOCKFILE_NAME),
    )
    .unwrap();
    assert!(run(&server.ctx(), dir.path(), &[]).await.ok);
    assert_eq!(server.creates(), 1);
}

#[tokio::test]
async fn prospective_collision_and_bad_creator_send_zero_requests() {
    let dir = fixture();
    png(dir.path(), "a.png");
    std::fs::write(
        dir.path().join("assets/a.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#,
    )
    .unwrap();
    let mut manifest = assets::load_manifest(dir.path()).unwrap();
    manifest.codegen.strip_extensions = true;
    write_manifest(dir.path(), &manifest);
    let server = MockServer::success();
    assert!(
        run(&server.ctx(), dir.path(), &[])
            .await
            .error
            .unwrap()
            .message
            .contains("collision")
    );
    manifest.creator.id = 0;
    write_manifest(dir.path(), &manifest);
    assert!(!run(&server.ctx(), dir.path(), &[]).await.ok);
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn changed_registered_image_is_reported_not_replaced() {
    let dir = fixture();
    png(dir.path(), "a.png");
    let server = MockServer::success();
    assert!(run(&server.ctx(), dir.path(), &[]).await.ok);
    image::RgbaImage::new(3, 3)
        .save(dir.path().join("assets/a.png"))
        .unwrap();
    assert!(!run_dry_run(dir.path(), &[]).ok);
    let out = run(&server.ctx(), dir.path(), &[]).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.data.unwrap()["changed"], json!(["assets/a.png"]));
    assert_eq!(server.creates(), 1);
    assert_eq!(
        assets::load_lockfile(dir.path()).unwrap().assets["assets/a.png"],
        101
    );
}

#[tokio::test]
async fn uncertain_server_failure_is_never_automatically_retried() {
    let dir = fixture();
    png(dir.path(), "a.png");
    let server = MockServer::new(|_, _, _| (503, json!({"message": "unknown server outcome"})));
    let mut ctx = server.ctx();
    ctx.retry = 3;
    assert_eq!(
        run(&ctx, dir.path(), &[]).await.error.unwrap().code,
        "publication_uncertain"
    );
    assert_eq!(
        run(&ctx, dir.path(), &[]).await.error.unwrap().code,
        "publication_uncertain"
    );
    assert_eq!(server.creates(), 1);
}

#[tokio::test]
async fn operation_receipt_write_failure_returns_known_id_for_offline_recovery() {
    let dir = fixture();
    png(dir.path(), "a.png");
    let root = dir.path().to_path_buf();
    let server = MockServer::new(move |request, body, creates| {
        if request.starts_with("POST ") {
            std::fs::rename(
                root.join(assets::LOCKFILE_NAME),
                root.join("saved-intent.toml"),
            )
            .unwrap();
            std::fs::create_dir(root.join(assets::LOCKFILE_NAME)).unwrap();
        }
        success_response(request, body, creates)
    });
    let out = run(&server.ctx(), dir.path(), &[]).await;
    assert_eq!(out.error.unwrap().code, "receipt_write_failed");
    assert!(out.hints.iter().any(|hint| hint.contains("op1")));
    std::fs::remove_dir(dir.path().join(assets::LOCKFILE_NAME)).unwrap();
    std::fs::rename(
        dir.path().join("saved-intent.toml"),
        dir.path().join(assets::LOCKFILE_NAME),
    )
    .unwrap();
    assert!(
        assets::load_lockfile(dir.path()).unwrap().pending["assets/a.png"]
            .operation_id
            .is_none()
    );
    assert!(recover_operation(dir.path(), "assets/a.png", "op1").ok);
    assert!(run(&server.ctx(), dir.path(), &[]).await.ok);
    assert_eq!(server.creates(), 1);
}

#[tokio::test]
async fn preprocessing_type_and_destination_errors_all_precede_mutations() {
    for invalid in ["z.svg", "z.png", "z.unknown"] {
        let dir = fixture();
        png(dir.path(), "a.png");
        std::fs::write(dir.path().join("assets").join(invalid), b"invalid data").unwrap();
        let server = MockServer::success();
        let out = run(&server.ctx(), dir.path(), &[]).await;
        assert!(!out.ok, "{invalid}");
        assert!(out.error.unwrap().message.contains(invalid));
        assert!(server.requests.lock().unwrap().is_empty());
    }
    let dir = fixture();
    png(dir.path(), "a.png");
    std::fs::create_dir_all(dir.path().join("generated/assets.luau")).unwrap();
    let server = MockServer::success();
    assert!(!run(&server.ctx(), dir.path(), &[]).await.ok);
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn image_resolution_failure_keeps_successful_receipt_without_new_upload() {
    let dir = fixture();
    png(dir.path(), "a.png");
    let server = MockServer::success();
    let out = run(&server.ctx(), dir.path(), &[]).await;
    assert!(out.ok);
    assert_eq!(
        out.data.unwrap()["unresolvedImages"],
        json!(["assets/a.png"])
    );
    let before = std::fs::read(dir.path().join(assets::LOCKFILE_NAME)).unwrap();
    assert!(!asset::image_id(101, None).ok);
    assert!(!asset::image_id(101, Some("invalid texture")).ok);
    assert_eq!(
        std::fs::read(dir.path().join(assets::LOCKFILE_NAME)).unwrap(),
        before
    );
    assert!(run(&server.ctx(), dir.path(), &[]).await.ok);
    assert_eq!(server.creates(), 1);
}

#[tokio::test]
async fn image_codegen_cannot_fail_for_missing_resolution_after_creating_assets() {
    let dir = fixture();
    png(dir.path(), "a.png");
    let mut manifest = assets::load_manifest(dir.path()).unwrap();
    manifest.codegen.image_ids = true;
    write_manifest(dir.path(), &manifest);
    let server = MockServer::success();
    assert_eq!(
        run(&server.ctx(), dir.path(), &[])
            .await
            .error
            .unwrap()
            .code,
        "image_resolution_required"
    );
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn output_cannot_overwrite_source_or_publication_receipts() {
    for (output_dir, basename) in [
        (".", assets::LOCKFILE_NAME),
        (".", assets::MANIFEST_NAME),
        ("assets", "a.png"),
    ] {
        let dir = fixture();
        png(dir.path(), "a.png");
        let original = std::fs::read(dir.path().join("assets/a.png")).unwrap();
        let mut manifest = assets::load_manifest(dir.path()).unwrap();
        manifest.codegen.output_path = output_dir.into();
        manifest.codegen.output_basename = Some(basename.into());
        write_manifest(dir.path(), &manifest);
        let server = MockServer::success();
        let out = run(&server.ctx(), dir.path(), &[]).await;
        assert!(!out.ok);
        assert!(out.error.unwrap().message.contains("overwrite"));
        assert!(server.requests.lock().unwrap().is_empty());
        assert_eq!(
            std::fs::read(dir.path().join("assets/a.png")).unwrap(),
            original
        );
        assert!(assets::load_manifest(dir.path()).is_ok());
    }
}
