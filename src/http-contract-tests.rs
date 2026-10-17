//! Offline wire contracts. All IDs and credentials are synthetic fixtures.

use crate::{auth::Auth, cmd, ctx::Ctx};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;

#[derive(Clone, Debug)]
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

enum Reply {
    Json(u16, Value),
    Raw(u16, String),
    Disconnect,
}

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn new(handler: impl Fn(usize) -> Reply + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let seen = requests.clone();
        let stop = stopped.clone();
        let thread = std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                };
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
                    if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let head = String::from_utf8_lossy(&bytes[..header_end]);
                let mut lines = head.lines();
                let mut start = lines.next().unwrap().split_whitespace();
                let method = start.next().unwrap().to_string();
                let target = start.next().unwrap().to_string();
                let headers: Vec<_> = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(key, value)| (key.to_string(), value.trim().to_string()))
                    .collect();
                let length = headers
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.parse::<usize>().unwrap())
                    .unwrap_or(0);
                assert!(
                    !headers
                        .iter()
                        .any(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding")),
                    "fixture requires a known body length"
                );
                while bytes.len() < header_end + length {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0, "request ended before body");
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let request = Request {
                    method,
                    target,
                    headers,
                    body: String::from_utf8(bytes[header_end..].to_vec()).unwrap(),
                };
                let count = {
                    let mut requests = seen.lock().unwrap();
                    requests.push(request);
                    requests.len()
                };
                let response = match handler(count) {
                    Reply::Json(status, body) => Some((status, body.to_string())),
                    Reply::Raw(status, body) => Some((status, body)),
                    Reply::Disconnect => None,
                };
                if let Some((status, body)) = response {
                    write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                }
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
        Self::new(|_| Reply::Json(200, json!({})))
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap(),
            auth: Auth::ApiKey("fixture-not-a-real-key".into()),
            retry: 2,
            api_host: self.url.clone(),
        }
    }

    fn captured(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let result = self.thread.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

#[tokio::test]
async fn standard_datastore_preserves_json_types_on_the_wire() {
    let server = Server::success();
    let ctx = server.ctx();
    let values = [
        json!({"coins": 100}),
        json!([1, true, null]),
        json!("hello"),
        json!(42),
        json!(1.25),
        json!(true),
        Value::Null,
        json!("{\"coins\":100}"),
    ];
    for value in &values {
        let out =
            cmd::datastore::set(&ctx, 123, "test", "player", None, &value.to_string(), false).await;
        assert!(out.ok, "{out:?}");
    }
    let requests = server.captured();
    assert_eq!(requests.len(), values.len());
    for (request, value) in requests.iter().zip(values) {
        assert_eq!(request.method, "PATCH");
        assert_eq!(
            serde_json::from_str::<Value>(&request.body).unwrap(),
            json!({"value": value})
        );
    }
}

#[tokio::test]
async fn datastore_create_flags_use_only_documented_query_parameters() {
    let server = Server::success();
    let ctx = server.ctx();
    for create in [true, false] {
        assert!(
            cmd::datastore::set(&ctx, 123, "test", "player", None, "1", create)
                .await
                .ok
        );
        assert!(
            cmd::datastore::set(&ctx, 123, "test", "player", Some("global"), "1", create)
                .await
                .ok
        );
        assert!(
            cmd::ordered_datastore::set(&ctx, 123, "test", "global", "player", 1, create)
                .await
                .ok
        );
    }
    let requests = server.captured();
    assert_eq!(requests.len(), 6);
    for (index, request) in requests.iter().enumerate() {
        let (base, parameter) = match index % 3 {
            0 => (
                "/cloud/v2/universes/123/data-stores/test/entries/player",
                "allowMissing",
            ),
            1 => (
                "/cloud/v2/universes/123/data-stores/test/scopes/global/entries/player",
                "allowMissing",
            ),
            _ => (
                "/ordered-data-stores/v1/universes/123/orderedDataStores/test/scopes/global/entries/player",
                "allow_missing",
            ),
        };
        assert_eq!(request.method, "PATCH");
        assert_eq!(
            request.target,
            if index < 3 {
                format!("{base}?{parameter}=true")
            } else {
                base.into()
            }
        );
        assert_eq!(request.header("if-none-match"), None);
        assert_eq!(request.header("roblox-entry-attributes"), None);
    }
}

async fn mutation(ctx: &Ctx, operation: &str) -> crate::output::Output {
    match operation {
        "increment" => {
            cmd::ordered_datastore::increment(ctx, 123, "test", "global", "player", 1).await
        }
        "message" => cmd::universe::publish_message(ctx, 123, "test", "{}").await,
        "create" => cmd::event::create(ctx, 123, "{}").await,
        "badge" => cmd::badge::create(ctx, 123, "Test", "Test", Some("user")).await,
        "restart" => cmd::server::restart(ctx, 123, false).await,
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn mutations_are_sent_once_after_transient_http_outcomes() {
    for status in [429, 500, 503] {
        for operation in ["increment", "message", "create", "badge", "restart"] {
            let server = Server::new(move |count| {
                Reply::Json(if count == 1 { status } else { 200 }, json!({}))
            });
            let out = mutation(&server.ctx(), operation).await;
            assert_eq!(server.captured().len(), 1, "{operation}, {status}: {out:?}");
            let error = out.error.unwrap();
            assert_eq!(error.code, "mutation_outcome_unknown");
            assert_eq!(error.status, Some(status));
            assert!(out.hints.iter().any(|hint| hint.contains("reconcile")));
            assert!(!out.hints.iter().any(|hint| hint.contains("--retry")));
        }
    }
}

#[tokio::test]
async fn mutations_are_sent_once_after_lost_responses() {
    for operation in ["increment", "message", "create", "badge", "restart"] {
        let server = Server::new(|count| {
            if count == 1 {
                Reply::Disconnect
            } else {
                Reply::Json(200, json!({}))
            }
        });
        let out = mutation(&server.ctx(), operation).await;
        assert_eq!(server.captured().len(), 1, "{operation}: {out:?}");
        let error = out.error.unwrap();
        assert_eq!(error.code, "mutation_outcome_unknown");
        assert_eq!(error.status, None);
        assert!(!out.hints.is_empty());
    }
}

#[tokio::test]
async fn replay_safe_reads_keep_bounded_retries() {
    for status in [429, 500, 0] {
        let server = Server::new(move |count| match (count, status) {
            (1, 0) => Reply::Disconnect,
            (1, status) => Reply::Json(status, json!({})),
            _ => Reply::Json(200, json!({"value": 1})),
        });
        let out = cmd::datastore::get(&server.ctx(), 123, "test", "player", None).await;
        assert!(out.ok, "{out:?}");
        assert_eq!(server.captured().len(), 2);
    }
    let server = Server::new(|_| Reply::Json(503, json!({})));
    let out = cmd::datastore::get(&server.ctx(), 123, "test", "player", None).await;
    assert!(!out.ok);
    assert_eq!(server.captured().len(), 3);
    assert!(!out.hints.is_empty());
}

#[tokio::test]
async fn restart_dry_run_is_a_read_only_forecast() {
    let server = Server::success();
    let out = cmd::server::restart(&server.ctx(), 123, true).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.command, "server restart");
    assert!(out.summary.contains("forecast"));
    let requests = server.captured();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].target,
        "/server-management/v1/universes/123/restarts:forecast"
    );
    assert!(requests[0].body.is_empty());

    let failed = Server::new(|_| Reply::Json(503, json!({})));
    let out = cmd::server::restart(&failed.ctx(), 123, true).await;
    assert!(!out.ok);
    assert_eq!(out.command, "server restart");
    let requests = failed.captured();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.method == "GET"
        && request.target == "/server-management/v1/universes/123/restarts:forecast"));
}

#[tokio::test]
async fn real_restart_omits_the_unsupported_dry_run_field() {
    let server = Server::success();
    let out = cmd::server::restart(&server.ctx(), 123, false).await;
    assert!(out.ok);
    assert_eq!(out.summary, "Requested a server restart");
    let requests = server.captured();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].target, "/cloud/v2/universes/123:restartServers");
    assert_eq!(
        serde_json::from_str::<Value>(&requests[0].body).unwrap(),
        json!({})
    );
}

#[tokio::test]
async fn badge_creation_uses_multipart_and_numeric_payment_sources() {
    let server = Server::success();
    for (source, expected) in [("user", "1"), ("group", "2")] {
        let out = cmd::badge::create(&server.ctx(), 123, "Test", "Description", Some(source)).await;
        assert!(out.ok, "{out:?}");
        assert!(out.hints.iter().any(|hint| hint.contains("Robux")));
        let requests = server.captured();
        let request = requests.last().unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/legacy-badges/v1/universes/123/badges");
        let content_type = request.header("content-type").unwrap();
        let boundary = content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        let parts: Vec<_> = request
            .body
            .split(&format!("--{boundary}"))
            .filter(|part| part.contains("Content-Disposition"))
            .collect();
        assert_eq!(parts.len(), 3);
        for (name, value) in [
            ("name", "Test"),
            ("description", "Description"),
            ("paymentSourceType", expected),
        ] {
            assert!(
                parts
                    .iter()
                    .any(|part| part.contains(&format!("name=\"{name}\"\r\n\r\n{value}\r\n"))),
                "{}",
                request.body
            );
        }
        assert!(!request.body.contains("name=\"paymentSource\""));
    }
}

#[tokio::test]
async fn invalid_badge_payment_sources_fail_before_any_paid_request() {
    let server = Server::success();
    for source in [None, Some("invalid"), Some(""), Some("1")] {
        let out = cmd::badge::create(&server.ctx(), 123, "Test", "Test", source).await;
        assert!(!out.ok, "{source:?}: {out:?}");
        assert!(out.error.is_some());
    }
    assert!(server.captured().is_empty());
}

#[tokio::test]
async fn thumbnail_delete_encodes_repeated_query_ids_without_a_body() {
    let server = Server::success();
    let out = cmd::thumbnail::delete(
        &server.ctx(),
        123,
        &["one".into(), "two".into(), "x&?# %".into()],
    )
    .await;
    assert!(out.ok, "{out:?}");
    let requests = server.captured();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "DELETE");
    assert_eq!(
        requests[0].target,
        "/thumbnail-personalization-api/v1/universes/123/thumbnails?homepageThumbnailIds=one&homepageThumbnailIds=two&homepageThumbnailIds=x%26%3F%23+%25"
    );
    assert!(requests[0].body.is_empty());
    assert_eq!(requests[0].header("content-type"), None);
}

#[tokio::test]
async fn empty_thumbnail_id_lists_fail_locally() {
    let server = Server::success();
    for ids in [vec![], vec!["".into()], vec!["one".into(), " ".into()]] {
        assert!(!cmd::thumbnail::delete(&server.ctx(), 123, &ids).await.ok);
    }
    assert!(server.captured().is_empty());
}

#[tokio::test]
async fn every_alert_operation_uses_the_case_sensitive_universe_resource() {
    let server = Server::success();
    let ctx = server.ctx();
    assert!(cmd::alerts::list(&ctx, "123", None).await.ok);
    assert!(cmd::alerts::create(&ctx, "123", "{}").await.ok);
    assert!(cmd::alerts::update(&ctx, "123", "456", "{}").await.ok);
    assert!(cmd::alerts::delete(&ctx, "123", "456").await.ok);
    assert!(cmd::alerts::incidents(&ctx, "123", "start", "end").await.ok);
    let requests = server.captured();
    assert_eq!(requests.len(), 5);
    let base = "/analytics-alert-control-plane/v1/resource/Universe/id/123/alerts";
    for (request, (method, suffix)) in requests.iter().zip([
        ("GET", ""),
        ("POST", ""),
        ("PATCH", "/456"),
        ("DELETE", "/456"),
        ("GET", "/incidents?startTime=start&endTime=end"),
    ]) {
        assert_eq!(request.method, method);
        assert_eq!(request.target, format!("{base}{suffix}"));
    }
}

#[tokio::test]
async fn ordered_list_uses_documented_sort_and_page_size_query_names() {
    let server = Server::success();
    for (ascending, page_size) in [(false, 25), (true, 1), (false, 100)] {
        let out = cmd::ordered_datastore::list(
            &server.ctx(),
            123,
            "board",
            "global",
            ascending,
            page_size,
        )
        .await;
        assert!(out.ok, "{out:?}");
    }
    let requests = server.captured();
    for (request, query) in requests.iter().zip([
        "order_by=desc&max_page_size=25",
        "order_by=asc&max_page_size=1",
        "order_by=desc&max_page_size=100",
    ]) {
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.target,
            format!(
                "/ordered-data-stores/v1/universes/123/orderedDataStores/board/scopes/global/entries?{query}"
            )
        );
    }
    assert_eq!(requests.len(), 3);
}

#[tokio::test]
async fn thumbnail_status_repeats_each_independently_encoded_operation_id() {
    let server = Server::success();
    for ids in [vec!["one".into()], vec!["one".into(), "two&?# %,+".into()]] {
        assert!(cmd::thumbnail::status(&server.ctx(), 123, &ids).await.ok);
    }
    let requests = server.captured();
    assert_eq!(requests.len(), 2);
    for (request, query) in requests.iter().zip([
        "operationIds=one",
        "operationIds=one&operationIds=two%26%3F%23+%25%2C%2B",
    ]) {
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.target,
            format!(
                "/thumbnail-personalization-api/v1/universes/123/thumbnails/uploads/status?{query}"
            )
        );
    }
}

#[tokio::test]
async fn place_history_uses_documented_page_size_query() {
    let server = Server::success();
    for limit in [1, 20, 100] {
        assert!(cmd::place::history(&server.ctx(), 456, limit).await.ok);
    }
    let requests = server.captured();
    assert_eq!(requests.len(), 3);
    for (request, size) in requests.iter().zip([1, 20, 100]) {
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.target,
            format!("/place-version-history-api/v1/456/history?pageSize={size}")
        );
    }
}

#[tokio::test]
async fn rollback_sends_full_version_resource_as_documented_multipart_form() {
    let server = Server::new(|_| {
        Reply::Json(
            200,
            json!({
                "path": "assets/123/versions/7", "published": false
            }),
        )
    });
    let out = cmd::asset::rollback(&server.ctx(), 123, 7).await;
    assert!(out.ok, "{out:?}");
    let requests = server.captured();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.target, "/assets/v1/assets/123/versions:rollback");
    assert!(
        request
            .header("content-type")
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    assert!(
        request
            .body
            .contains("name=\"assetVersion\"\r\n\r\nassets/123/versions/7\r\n")
    );
    assert_eq!(request.body.matches("Content-Disposition:").count(), 1);
    assert_eq!(out.data.as_ref().unwrap()["path"], "assets/123/versions/7");
    assert!(!out.summary.contains("operation"));
}

#[tokio::test]
async fn rollback_does_not_retry_an_uncertain_mutation() {
    let server = Server::new(|_| Reply::Json(503, json!({"message": "fixture unavailable"})));
    let out = cmd::asset::rollback(&server.ctx(), 123, 7).await;
    assert!(!out.ok);
    assert_eq!(out.error.unwrap().code, "mutation_outcome_unknown");
    assert!(!out.hints.is_empty());
    assert_eq!(server.captured().len(), 1);
}

#[tokio::test]
async fn rollback_rejects_missing_or_wrong_asset_version_receipts() {
    for body in [
        json!({}),
        json!({"path": "assets/456/versions/7"}),
        json!({"path": "assets/123/versions/nope"}),
    ] {
        let server = Server::new(move |_| Reply::Json(200, body.clone()));
        let out = cmd::asset::rollback(&server.ctx(), 123, 7).await;
        assert!(!out.ok);
        assert_eq!(out.error.unwrap().code, "invalid_asset_version");
        assert!(!out.hints.is_empty());
        assert_eq!(server.captured().len(), 1);
    }
}

#[tokio::test]
async fn documented_thumbnail_file_operation_map_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("thumb.png");
    std::fs::write(&file, "synthetic image bytes").unwrap();
    let server = Server::new(|_| {
        Reply::Json(
            200,
            json!({
                "fileToOperationIdDict": {"thumb.png": "op-1", "other.png": "op-2"}
            }),
        )
    });
    let out = cmd::thumbnail::upload(&server.ctx(), 123, &[file]).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.data.unwrap()["operationIds"], json!(["op-1", "op-2"]));
    assert!(out.hints.iter().any(|h| h.contains("op-1")));
}

#[tokio::test]
async fn documented_game_server_logs_preserve_entries_and_limit() {
    let server = Server::new(|_| {
        Reply::Json(
            200,
            json!({
                "gameServerLogs": [{"message": "hello"}, {"message": "world"}]
            }),
        )
    });
    let out = cmd::server::logs(&server.ctx(), 123, 456, 7, "fixture-job", 1, false).await;
    assert!(out.ok, "{out:?}");
    let data = out.data.unwrap();
    assert_eq!(data["total"], 2);
    assert_eq!(data["shown"], 1);
    assert_eq!(data["entries"], json!([{"message": "hello"}]));
    assert!(!out.hints.is_empty());
}

#[tokio::test]
async fn completed_asset_operation_errors_are_not_success() {
    let server = Server::new(|_| {
        Reply::Json(
            200,
            json!({
                "done": true, "error": {"code": 3, "message": "rejected"}
            }),
        )
    });
    let out = cmd::asset::operation(&server.ctx(), "op-1").await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(out.error.unwrap().code, "operation_failed");
    assert!(!out.hints.is_empty());
}

#[tokio::test]
async fn asset_submissions_reject_missing_and_malformed_operation_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.fbx");
    std::fs::write(&model, "synthetic model").unwrap();
    for body in [
        json!({}),
        json!({"path": ""}),
        json!({"path": "operations/"}),
        json!({"path": "operations/a/b"}),
    ] {
        let server = Server::new(move |_| Reply::Json(200, body.clone()));
        for out in [
            cmd::asset::create(&server.ctx(), &model, "Model", "", Some(123), None, None).await,
            cmd::asset::update(&server.ctx(), 456, &model, None, None, None, None, None).await,
        ] {
            assert!(!out.ok, "{out:?}");
            assert_eq!(out.error.unwrap().code, "operation_id_missing");
            assert!(!out.hints.is_empty());
        }
        assert_eq!(server.captured().len(), 2);
    }
}

#[tokio::test]
async fn documented_collection_shapes_have_truthful_summaries() {
    let server = Server::new(|count| {
        Reply::Json(
            200,
            match count {
                1 => json!({"placeVersions": [{"versionNumber": 7}, {"versionNumber": 6}]}),
                2 => json!([{"path": "assets/123/versions/7"}]),
                3 => json!([{"id": "fixture-1"}, {"id": "fixture-2"}]),
                _ => unreachable!(),
            },
        )
    });
    for (out, summary) in [
        (
            cmd::place::history(&server.ctx(), 456, 20).await,
            "Fetched 2 versions",
        ),
        (
            cmd::asset::versions(&server.ctx(), 123, 20).await,
            "Fetched 1 versions",
        ),
        (
            cmd::alerts::incidents(&server.ctx(), "123", "start", "end").await,
            "Fetched 2 incidents",
        ),
    ] {
        assert!(out.ok, "{out:?}");
        assert!(out.summary.starts_with(summary), "{out:?}");
    }
}

#[tokio::test]
async fn place_publish_parses_the_documented_version_number_object() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("fixture.rbxlx");
    std::fs::write(&file, "synthetic place bytes").unwrap();
    let server = Server::new(|_| Reply::Json(200, json!({"versionNumber": 7})));
    let out = cmd::place::publish(&server.ctx(), 123, 456, &file, false).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.data.unwrap()["version"], 7);
    assert!(out.summary.contains("version 7"));
}

#[tokio::test]
async fn required_json_mutations_reject_malformed_and_empty_success_bodies() {
    for body in ["not json", ""] {
        let server = Server::new(move |_| Reply::Raw(200, body.into()));
        let mut ctx = server.ctx();
        ctx.auth = Auth::OAuth("synthetic-oauth-token".into());
        for out in [
            cmd::event::create(&ctx, 123, "{}").await,
            cmd::event::update(&ctx, 456, "{}").await,
            cmd::badge::create(&ctx, 123, "Fixture", "", Some("user")).await,
            cmd::badge::update(&ctx, 456, Some("Fixture"), None, None).await,
            cmd::datastore::set(&ctx, 123, "store", "entry", None, "1", false).await,
            cmd::ordered_datastore::set(&ctx, 123, "store", "global", "entry", 1, false).await,
            cmd::ordered_datastore::increment(&ctx, 123, "store", "global", "entry", 1).await,
            cmd::server::restart(&ctx, 123, false).await,
            cmd::config::patch_draft(&ctx, 123, "repo", "{}").await,
            cmd::config::overwrite(&ctx, 123, "repo", "{}").await,
            cmd::config::publish(&ctx, 123, "repo").await,
            cmd::config::restore(&ctx, 123, "repo", "rev").await,
            cmd::alerts::create(&ctx, "123", "{}").await,
            cmd::alerts::update(&ctx, "123", "456", "{}").await,
            cmd::analytics::metrics(&ctx, 123, "{}").await,
            cmd::permissions::grant(&ctx, &[456], "user", 123, "use").await,
            cmd::monetization::gamepass_create(&ctx, 123, "Fixture", None, None, None).await,
            cmd::monetization::devproduct_create(&ctx, 123, "Fixture", None, None, None).await,
        ] {
            assert!(!out.ok, "{} accepted malformed required JSON", out.command);
            assert_eq!(out.error.as_ref().unwrap().code, "invalid_json", "{out:?}");
            assert!(!out.hints.is_empty());
        }
        assert_eq!(server.captured().len(), 18);
    }
}

#[tokio::test]
async fn documented_no_content_mutations_remain_successful() {
    let server = Server::new(|_| Reply::Raw(204, String::new()));
    let ctx = server.ctx();
    for out in [
        cmd::event::delete(&ctx, 123).await,
        cmd::alerts::delete(&ctx, "123", "456").await,
        cmd::datastore::delete(&ctx, 123, "store", "entry", None).await,
        cmd::ordered_datastore::delete(&ctx, 123, "store", "global", "entry").await,
        cmd::monetization::gamepass_update(&ctx, 123, 456, Some("Fixture"), None, None, None).await,
    ] {
        assert!(out.ok, "{out:?}");
    }
    assert_eq!(server.captured().len(), 5);
}

#[tokio::test]
async fn product_creates_never_report_unknown_successful_ids() {
    for body in [
        json!({}),
        json!({"id": false}),
        json!({"productId": 0}),
        json!({"gamePassId": ""}),
    ] {
        let server = Server::new(move |_| Reply::Json(200, body.clone()));
        for out in [
            cmd::monetization::gamepass_create(&server.ctx(), 123, "Fixture", None, None, None)
                .await,
            cmd::monetization::devproduct_create(&server.ctx(), 123, "Fixture", None, None, None)
                .await,
        ] {
            assert!(!out.ok, "{out:?}");
            assert_eq!(out.error.unwrap().code, "resource_id_missing");
            assert!(!out.hints.is_empty());
        }
        assert_eq!(server.captured().len(), 2);
    }
}

#[tokio::test]
async fn local_and_http_failures_have_actionable_hints_in_both_formats() {
    let server = Server::success();
    let ctx = server.ctx();
    let http_server =
        Server::new(|_| Reply::Json(400, json!({"message": "invalid fixture request"})));
    let invalid_json_server = Server::new(|_| Reply::Raw(200, "not-json-fixture".into()));
    for out in [
        cmd::analytics::metrics(&ctx, 123, "nope").await,
        cmd::thumbnail::upload(&ctx, 123, &[]).await,
        cmd::datastore::get(&http_server.ctx(), 123, "test", "player", None).await,
        cmd::datastore::get(&invalid_json_server.ctx(), 123, "test", "player", None).await,
    ] {
        assert!(!out.ok);
        let json = serde_json::to_value(&out).unwrap();
        let hints = json["hints"]
            .as_array()
            .expect("failure must serialize hints");
        assert!(!hints.is_empty());
        for hint in hints {
            let hint = hint.as_str().unwrap();
            assert!(!hint.trim().is_empty());
            assert!(out.render_text().contains(hint));
        }
    }
}
