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

    fn json_part(&self, name: &str) -> Value {
        let boundary = self
            .header("content-type")
            .unwrap()
            .split("boundary=")
            .nth(1)
            .unwrap();
        let marker = format!("name=\"{name}\"");
        let part = self
            .body
            .split(&format!("--{boundary}"))
            .find(|part| part.contains(&marker))
            .unwrap();
        serde_json::from_str(part.split_once("\r\n\r\n").unwrap().1.trim_end()).unwrap()
    }
}

enum Reply {
    Json(u16, Value),
    Raw(u16, String),
    /// Status plus extra raw response headers, one `Name: value` per entry.
    RawWithHeaders(u16, Vec<String>, String),
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
                    Reply::Json(status, body) => Some((status, Vec::new(), body.to_string())),
                    Reply::Raw(status, body) => Some((status, Vec::new(), body)),
                    Reply::RawWithHeaders(status, headers, body) => Some((status, headers, body)),
                    Reply::Disconnect => None,
                };
                if let Some((status, extra, body)) = response {
                    let extra: String =
                        extra.iter().map(|header| format!("{header}\r\n")).collect();
                    write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nRetry-After: 0\r\n{extra}Connection: close\r\n\r\n{body}", body.len()).unwrap();
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
            // Match the production client: never follow redirects.
            client: crate::auth::http_client_builder()
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
    let server = Server::new(|_| {
        Reply::Json(
            200,
            json!({
                "value": 0,
                "attributes": {"rank": 1},
                "users": ["users/7"],
                "etag": "etag-1"
            }),
        )
    });
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
        let out = cmd::datastore::set(
            &ctx,
            123,
            "test",
            "player",
            None,
            &value.to_string(),
            false,
            cmd::datastore::ValueUpdate::default(),
        )
        .await;
        assert!(out.ok, "{out:?}");
    }
    let requests = server.captured();
    assert_eq!(requests.len(), values.len() * 2);
    for (pair, value) in requests.chunks(2).zip(values) {
        assert_eq!(pair[0].method, "GET");
        assert_eq!(pair[1].method, "PATCH");
        assert_eq!(
            serde_json::from_str::<Value>(&pair[1].body).unwrap(),
            json!({
                "value": value,
                "attributes": {"rank": 1},
                "users": ["users/7"],
                "etag": "etag-1"
            })
        );
    }
}

#[tokio::test]
async fn datastore_create_flags_use_only_documented_query_parameters() {
    let server = Server::new(|_| {
        Reply::Json(
            200,
            json!({"value": 1, "attributes": {}, "users": [], "etag": "etag-1"}),
        )
    });
    let ctx = server.ctx();
    for create in [true, false] {
        assert!(
            cmd::datastore::set(
                &ctx,
                123,
                "test",
                "player",
                None,
                "1",
                create,
                cmd::datastore::ValueUpdate::default(),
            )
            .await
            .ok
        );
        assert!(
            cmd::datastore::set(
                &ctx,
                123,
                "test",
                "player",
                Some("global"),
                "1",
                create,
                cmd::datastore::ValueUpdate::default(),
            )
            .await
            .ok
        );
        assert!(
            cmd::ordered_datastore::set(&ctx, 123, "test", "global", "player", 1, create)
                .await
                .ok
        );
    }
    let patches: Vec<_> = server
        .captured()
        .into_iter()
        .filter(|request| request.method == "PATCH")
        .collect();
    assert_eq!(patches.len(), 6);
    for (index, request) in patches.iter().enumerate() {
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
        if index % 3 != 2 {
            let body: Value = serde_json::from_str(&request.body).unwrap();
            assert_eq!(body["etag"], "etag-1");
            assert!(body.get("attributes").is_some());
            assert!(body.get("users").is_some());
        }
    }
}

fn paid_badge<'a>(
    name: &'a str,
    description: &'a str,
    payment_source: Option<&'a str>,
) -> cmd::badge::BadgeCreate<'a> {
    cmd::badge::BadgeCreate {
        name,
        description,
        payment_source,
        expected_cost: Some(100),
        active: Some(true),
        icon: None,
    }
}

async fn mutation(ctx: &Ctx, operation: &str) -> crate::output::Output {
    match operation {
        "increment" => {
            cmd::ordered_datastore::increment(ctx, 123, "test", "global", "player", 1).await
        }
        "message" => cmd::universe::publish_message(ctx, 123, "test", "{}").await,
        "create" => cmd::event::create(ctx, 123, "{}").await,
        "badge" => cmd::badge::create(ctx, 123, paid_badge("Test", "Test", Some("user"))).await,
        "restart" => cmd::server::restart(ctx, 123, false).await,
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn mutations_are_sent_once_after_transient_http_outcomes() {
    // 408 is included: a timeout response does not prove the write did not land.
    for status in [408, 429, 500, 503] {
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
async fn read_only_queries_are_not_classified_as_unknown_mutations() {
    let query = r#"{"metric":"DailyActiveUsers","granularity":"OneDay","startTime":"2026-10-01T00:00:00Z","endTime":"2026-10-08T00:00:00Z"}"#;
    for status in [408, 429, 500, 503] {
        let server = Server::new(move |_| Reply::Json(status, json!({"message": "fixture"})));
        let out = cmd::analytics::metrics(&server.ctx(), 123, query).await;
        assert_eq!(server.captured().len(), 1, "{status}: {out:?}");
        let error = out.error.unwrap();
        assert_eq!(error.code, "read_outcome_unknown", "{status}");
        assert_eq!(error.status, Some(status));
        assert!(
            !out.hints
                .iter()
                .any(|hint| hint.to_lowercase().contains("reconcile"))
        );
        assert!(
            out.hints
                .iter()
                .any(|hint| hint.contains("read-only query"))
        );
    }
    let lost = Server::new(|_| Reply::Disconnect);
    let out = cmd::analytics::metrics(&lost.ctx(), 123, query).await;
    assert_eq!(lost.captured().len(), 1);
    let error = out.error.unwrap();
    assert_eq!(error.code, "read_outcome_unknown");
    assert_eq!(error.status, None);
    assert!(
        out.hints
            .iter()
            .any(|hint| hint.contains("read-only query"))
    );
}

#[tokio::test]
async fn redirects_never_replay_mutations_or_forward_the_api_key() {
    // Same origin: a followed 307 would send the increment POST twice.
    let same = Server::new(|_| {
        Reply::RawWithHeaders(307, vec!["Location: /redirected".into()], String::new())
    });
    let out =
        cmd::ordered_datastore::increment(&same.ctx(), 123, "test", "global", "player", 1).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(same.captured().len(), 1);
    let error = out.error.unwrap();
    assert_eq!(error.code, "unexpected_redirect");
    assert_eq!(error.status, Some(307));
    assert!(!out.hints.is_empty());

    // Cross origin: a followed redirect would forward x-api-key to the target.
    let target = Server::new(|_| Reply::Json(200, json!({"value": 1})));
    let origin = Server::new({
        let location = format!("Location: {}/redirected", target.url);
        move |_| Reply::RawWithHeaders(307, vec![location.clone()], String::new())
    });
    let out =
        cmd::ordered_datastore::increment(&origin.ctx(), 123, "test", "global", "player", 1).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(origin.captured().len(), 1);
    assert!(target.captured().is_empty());
    assert_eq!(out.error.unwrap().code, "unexpected_redirect");

    // Publication requests use send_once and must refuse redirects too.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("place.rbxl");
    std::fs::write(&file, b"rbxl").unwrap();
    let publish = Server::new(|_| {
        Reply::RawWithHeaders(307, vec!["Location: /redirected".into()], String::new())
    });
    let out = cmd::place::publish(&publish.ctx(), 123, 456, &file, true).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(publish.captured().len(), 1);
    assert_eq!(out.error.unwrap().code, "unexpected_redirect");
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
        let out = cmd::badge::create(
            &server.ctx(),
            123,
            paid_badge("Test", "Description", Some(source)),
        )
        .await;
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
        assert_eq!(parts.len(), 5);
        for (name, value) in [
            ("name", "Test"),
            ("description", "Description"),
            ("paymentSourceType", expected),
            ("expectedCost", "100"),
            ("isActive", "true"),
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
        let out = cmd::badge::create(&server.ctx(), 123, paid_badge("Test", "Test", source)).await;
        assert!(!out.ok, "{source:?}: {out:?}");
        assert!(out.error.is_some());
    }
    assert!(server.captured().is_empty());
}

#[tokio::test]
async fn thumbnail_delete_encodes_repeated_query_ids_without_a_body() {
    let server =
        Server::new(|_| Reply::Json(200, json!({"one": true, "two": true, "x&?# %": true})));
    let out = cmd::thumbnail::delete(
        &server.ctx(),
        123,
        &["one".into(), "two".into(), "x&?# %".into()],
    )
    .await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.summary, "Deleted 3 thumbnails");
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
async fn thumbnail_delete_reports_the_documented_per_id_results() {
    // All true: an unqualified success is correct.
    let all = Server::new(|_| Reply::Json(200, json!({"a": true, "b": true})));
    let out = cmd::thumbnail::delete(&all.ctx(), 123, &["a".into(), "b".into()]).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.data.unwrap()["partial"], false);

    // Mixed: one ID was not confirmed deleted.
    let mixed = Server::new(|_| Reply::Json(200, json!({"a": true, "b": false})));
    let out = cmd::thumbnail::delete(&mixed.ctx(), 123, &["a".into(), "b".into()]).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(mixed.captured().len(), 1);
    assert!(out.summary.contains("partial"), "{}", out.summary);
    assert!(out.hints.iter().any(|hint| hint.contains("notDeleted")));
    let data = out.data.unwrap();
    assert_eq!(data["removed"], 1);
    assert_eq!(data["partial"], true);
    assert_eq!(data["deleted"], json!(["a"]));
    assert_eq!(data["notDeleted"], json!(["b"]));
    assert_eq!(data["unconfirmed"], json!([]));

    // A missing ID cannot be counted as deleted.
    let missing = Server::new(|_| Reply::Json(200, json!({"a": true})));
    let out = cmd::thumbnail::delete(&missing.ctx(), 123, &["a".into(), "b".into()]).await;
    assert!(out.ok, "{out:?}");
    let data = out.data.unwrap();
    assert_eq!(data["removed"], 1);
    assert_eq!(data["unconfirmed"], json!(["b"]));

    // Nothing confirmed: no unqualified success is possible.
    let none = Server::new(|_| Reply::Json(200, json!({"a": false, "b": false})));
    let out = cmd::thumbnail::delete(&none.ctx(), 123, &["a".into(), "b".into()]).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(none.captured().len(), 1);
    assert_eq!(out.error.unwrap().code, "thumbnail_delete_failed");

    // Malformed bodies are never reported as success, and the raw body is
    // preserved for reconciliation.
    for body in [json!([true, false]), json!(null), json!("ok")] {
        let label = body.clone();
        let bad = Server::new(move |_| Reply::Json(200, body.clone()));
        let out = cmd::thumbnail::delete(&bad.ctx(), 123, &["a".into()]).await;
        assert!(!out.ok, "{label}: {out:?}");
        assert_eq!(bad.captured().len(), 1);
        assert_eq!(out.error.unwrap().code, "thumbnail_response_invalid");
        assert_eq!(out.data.unwrap()["response"], label);
    }

    // The per-ID receipt survives a failure so the caller can reconcile.
    let none2 = Server::new(|_| Reply::Json(200, json!({"a": false, "b": false})));
    let out = cmd::thumbnail::delete(&none2.ctx(), 123, &["a".into(), "b".into()]).await;
    assert!(!out.ok);
    let data = out.data.unwrap();
    assert_eq!(data["notDeleted"], json!(["a", "b"]));
    assert_eq!(data["results"]["b"], false);
    assert!(!out.hints.is_empty());
    assert!(
        out.hints
            .iter()
            .any(|hint| hint.contains("sent at most once"))
    );
    assert!(
        !out.hints
            .iter()
            .any(|hint| hint.contains("correct the input"))
    );

    // Non-boolean per-ID values stay unconfirmed rather than counting as gone.
    let typed = Server::new(|_| Reply::Json(200, json!({"a": "true"})));
    let out = cmd::thumbnail::delete(&typed.ctx(), 123, &["a".into()]).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(out.error.unwrap().code, "thumbnail_delete_failed");
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
            None,
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
        assert!(
            cmd::place::history(&server.ctx(), 456, limit, None)
                .await
                .ok
        );
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
async fn reviewer_thumbnail_upload_rejects_blank_receipts_from_every_supported_shape() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("thumb.png");
    std::fs::write(&file, "synthetic image bytes").unwrap();
    for body in [
        json!({"operationIds": [""]}),
        json!({"paths": ["  "]}),
        json!({"operations": [{"path": "operations/"}]}),
        json!({"fileToOperationIdDict": {"thumb.png": "operations/ "}}),
    ] {
        let server = Server::new(move |_| Reply::Json(200, body.clone()));
        let out = cmd::thumbnail::upload(&server.ctx(), 123, std::slice::from_ref(&file)).await;
        assert!(!out.ok, "{out:?}");
        assert_eq!(out.error.unwrap().code, "operation_id_missing");
        assert_eq!(server.captured().len(), 1);
    }
    let server = Server::new(|_| {
        Reply::Json(
            200,
            json!({
                "operationIds": ["", "operations/op-1", "  "],
                "operations": [{"path": "operations/"}],
                "fileToOperationIdDict": {"thumb.png": "op-2"},
            }),
        )
    });
    let out = cmd::thumbnail::upload(&server.ctx(), 123, &[file]).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.data.unwrap()["operationIds"], json!(["op-1", "op-2"]));
}

#[tokio::test]
async fn reviewer_permission_grants_preserve_partial_success_and_never_replay_the_batch() {
    let response = json!({"successAssetIds": [123], "errors": [{"assetId": 456, "code": "CannotManageAsset"}]});
    let reply = response.clone();
    let server = Server::new(move |_| Reply::Json(200, reply.clone()));
    let mut ctx = server.ctx();
    ctx.retry = 3;
    let out = cmd::permissions::grant(&ctx, &[123, 456], "user", Some(789), "use").await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(out.error.as_ref().unwrap().code, "permission_grant_failed");
    assert_eq!(out.data.as_ref().unwrap()["successAssetIds"], json!([123]));
    assert_eq!(out.data.as_ref().unwrap()["response"], response);
    assert!(!out.hints.is_empty());
    assert_eq!(server.captured().len(), 1);

    let server =
        Server::new(|_| Reply::Json(200, json!({"successAssetIds": [123, 456], "errors": null})));
    assert!(
        cmd::permissions::grant(&server.ctx(), &[123, 456], "user", Some(789), "use")
            .await
            .ok
    );
}

#[tokio::test]
async fn reviewer_permission_grants_reject_unproven_or_malformed_success_receipts() {
    for body in [
        json!({}),
        json!({"successAssetIds": [123]}),
        json!({"successAssetIds": [123, 999], "errors": []}),
        json!({"successAssetIds": ["123", 456], "errors": []}),
        json!({"successAssetIds": [123, 456], "errors": "not-an-array"}),
    ] {
        let server = Server::new(move |_| Reply::Json(200, body.clone()));
        let out =
            cmd::permissions::grant(&server.ctx(), &[123, 456], "user", Some(789), "use").await;
        assert!(!out.ok, "{out:?}");
        assert_eq!(out.error.unwrap().code, "invalid_permissions_response");
        assert_eq!(server.captured().len(), 1);
    }
}

#[tokio::test]
async fn server_list_surfaces_provider_declared_partial_results() {
    // Normal responses keep the existing success behavior.
    for flags in [
        json!({}),
        json!({"activeServersFetchError": false, "shutdownServersFetchError": false}),
        json!({"activeServersFetchError": null, "shutdownServersFetchError": null}),
    ] {
        let mut body = json!({"gameServers": [{"jobId": "active-job"}], "nextPageToken": "next"});
        body.as_object_mut()
            .unwrap()
            .extend(flags.as_object().unwrap().clone());
        let server = Server::new(move |_| Reply::Json(200, body.clone()));
        let out = cmd::server::list(&server.ctx(), 123, 456, 7, None).await;
        assert!(out.ok, "{flags}: {out:?}");
        assert_eq!(out.summary, "Fetched 1 game servers", "{flags}");
        let data = out.data.unwrap();
        assert_eq!(data["partial"], false, "{flags}");
        assert_eq!(data["missingSources"], json!([]), "{flags}");
    }

    // Each flag independently, then both together.
    for (flag, missing) in [
        ("shutdownServersFetchError", "shutdown"),
        ("activeServersFetchError", "active"),
    ] {
        let mut body = json!({"gameServers": [{"jobId": "active-job"}]});
        body.as_object_mut()
            .unwrap()
            .insert(flag.into(), json!(true));
        let server = Server::new(move |_| Reply::Json(200, body.clone()));
        let out = cmd::server::list(&server.ctx(), 123, 456, 7, None).await;
        assert!(out.ok, "{flag}: {out:?}");
        assert!(out.summary.contains("partial"), "{flag}: {}", out.summary);
        assert!(
            out.hints.iter().any(|hint| hint.contains("incomplete")),
            "{flag}: {out:?}"
        );
        let data = out.data.unwrap();
        assert_eq!(data["partial"], true);
        assert_eq!(data["missingSources"], json!([missing]));
        assert_eq!(data[flag], true);
        // Available rows stay usable for reconciliation.
        assert_eq!(data["count"], 1);
        assert_eq!(data["jobIds"], json!(["active-job"]));
    }

    let both = Server::new(|_| {
        Reply::Json(
            200,
            json!({
                "gameServers": [{"jobId": "active-job"}],
                "shutdownServersFetchError": true,
                "activeServersFetchError": true
            }),
        )
    });
    let out = cmd::server::list(&both.ctx(), 123, 456, 7, None).await;
    assert!(out.ok, "{out:?}");
    assert!(out.summary.contains("partial"));
    let data = out.data.unwrap();
    assert_eq!(data["partial"], true);
    assert_eq!(data["missingSources"], json!(["active", "shutdown"]));
    assert_eq!(data["response"]["activeServersFetchError"], true);
    assert_eq!(data["response"]["shutdownServersFetchError"], true);
    assert_eq!(both.captured().len(), 1);
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
    let out = cmd::server::logs(&server.ctx(), 123, 456, 7, "fixture-job", 1, false, None).await;
    assert!(out.ok, "{out:?}");
    let requests = server.captured();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].target.contains("MaxPageSize=1"));
    assert!(!requests[0].target.contains("PageToken="));
    let data = out.data.unwrap();
    assert_eq!(data["entries"].as_array().unwrap().len(), 2);
    assert!(data["nextPageToken"].is_null());
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
async fn immediately_failed_asset_submissions_preserve_receipts_but_never_report_success() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.fbx");
    std::fs::write(&model, "synthetic model").unwrap();
    let server = Server::new(|_| {
        Reply::Json(
            200,
            json!({
                "path": "operations/op-failed", "done": true,
                "error": {"code": 3, "message": "fixture rejected"},
            }),
        )
    });
    for out in [
        cmd::asset::create(&server.ctx(), &model, "Model", "", Some(123), None, None).await,
        cmd::asset::update(
            &server.ctx(),
            456,
            Some(&model),
            None,
            None,
            None,
            None,
            &[],
        )
        .await,
        cmd::asset::update(
            &server.ctx(),
            456,
            None,
            None,
            Some("Renamed"),
            None,
            None,
            &[],
        )
        .await,
    ] {
        assert!(!out.ok, "{out:?}");
        assert_eq!(out.error.unwrap().code, "operation_failed");
        assert_eq!(out.data.unwrap()["operationId"], "op-failed");
        assert!(out.hints.iter().any(|h| h.contains("op-failed")));
    }
    assert_eq!(server.captured().len(), 3);
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
            cmd::asset::update(
                &server.ctx(),
                456,
                Some(&model),
                None,
                None,
                None,
                None,
                &[],
            )
            .await,
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
            cmd::place::history(&server.ctx(), 456, 20, None).await,
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
    assert_eq!(
        server.captured()[0].header("content-type"),
        Some("application/xml")
    );
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
            cmd::badge::create(&ctx, 123, paid_badge("Fixture", "", Some("user"))).await,
            cmd::badge::update(&ctx, 456, Some("Fixture"), None, None).await,
            cmd::datastore::set(&ctx, 123, "store", "entry", None, "1", false, cmd::datastore::ValueUpdate::default()).await,
            cmd::ordered_datastore::set(&ctx, 123, "store", "global", "entry", 1, false).await,
            cmd::ordered_datastore::increment(&ctx, 123, "store", "global", "entry", 1).await,
            cmd::server::restart(&ctx, 123, false).await,
            cmd::config::patch_draft(&ctx, 123, "InExperienceConfig", "{}").await,
            cmd::config::overwrite(&ctx, 123, "InExperienceConfig", "{}").await,
            cmd::config::publish(&ctx, 123, "InExperienceConfig", Some("draft-hash"), None, None).await,
            cmd::config::restore(&ctx, 123, "InExperienceConfig", "rev").await,
            cmd::alerts::create(&ctx, "123", "{}").await,
            cmd::alerts::update(&ctx, "123", "456", "{}").await,
            cmd::analytics::metrics(&ctx, 123, "{\"metric\":\"DailyActiveUsers\",\"granularity\":\"OneDay\",\"startTime\":\"2026-10-01T00:00:00Z\",\"endTime\":\"2026-10-08T00:00:00Z\"}").await,
            cmd::permissions::grant(&ctx, &[456], "user", Some(123), "use").await,
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
        cmd::monetization::gamepass_update(&ctx, 123, 456, Some("Fixture"), None, None, None, None)
            .await,
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
async fn analytics_api_key_errors_are_not_unconditionally_oauth_required() {
    let denied = Server::new(|_| Reply::Json(403, json!({"message": "permission denied"})));
    let query = r#"{"metric":"DailyActiveUsers","granularity":"OneDay","startTime":"2026-10-01T00:00:00Z","endTime":"2026-10-08T00:00:00Z"}"#;
    let out = cmd::analytics::metrics(&denied.ctx(), 123, query).await;
    assert!(!out.ok);
    assert_eq!(out.error.unwrap().code, "http_403");
    assert!(
        out.hints
            .iter()
            .any(|h| h.contains("universe.analytics:read"))
    );

    let malformed = Server::new(|_| Reply::Raw(200, "not json".into()));
    let out = cmd::analytics::metrics(&malformed.ctx(), 123, query).await;
    assert_eq!(out.error.unwrap().code, "invalid_json");
}

#[tokio::test]
async fn asset_content_update_omits_metadata_mask_and_undocumented_parts() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.fbx");
    std::fs::write(&model, "synthetic Model content").unwrap();
    let server = Server::new(|_| Reply::Json(200, json!({"path": "operations/op-1"})));
    let out = cmd::asset::update(
        &server.ctx(),
        123,
        Some(&model),
        None,
        None,
        None,
        None,
        &[],
    )
    .await;
    assert!(out.ok, "{out:?}");
    let requests = server.captured();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "PATCH");
    assert_eq!(requests[0].target, "/assets/v1/assets/123");
    assert_eq!(requests[0].json_part("request"), json!({"assetId": "123"}));
    assert_eq!(requests[0].body.matches("Content-Disposition:").count(), 2);
    assert!(requests[0].body.contains("name=\"fileContent\""));
}

#[tokio::test]
async fn asset_metadata_updates_use_only_documented_json_fields_and_parts() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.fbx");
    std::fs::write(&model, "synthetic Model content").unwrap();
    for content in [false, true] {
        let server = Server::new(move |_| {
            Reply::Json(
                200,
                if content {
                    json!({"path": "operations/op-1"})
                } else {
                    json!({"previews": [{"asset": "assets/789"}, {"asset": "assets/790"}]})
                },
            )
        });
        let out = cmd::asset::update(
            &server.ctx(),
            123,
            content.then_some(model.as_path()),
            None,
            Some("Renamed"),
            Some(""),
            Some("assets/456"),
            &["assets/789".into(), "assets/790".into()],
        )
        .await;
        assert!(out.ok, "{out:?}");
        let requests = server.captured();
        assert_eq!(requests.len(), 1);
        let req = &requests[0];
        assert_eq!(req.method, "PATCH");
        assert_eq!(
            req.target,
            "/assets/v1/assets/123?updateMask=displayName%2Cdescription%2Cicon%2Cpreviews"
        );
        assert_eq!(
            req.json_part("request"),
            json!({
                "assetId": "123", "displayName": "Renamed", "description": "",
                "icon": "assets/456", "previews": [{"asset": "assets/789"}, {"asset": "assets/790"}],
            })
        );
        assert_eq!(
            req.body.matches("Content-Disposition:").count(),
            if content { 2 } else { 1 }
        );
        assert_eq!(req.body.contains("name=\"fileContent\""), content);
        assert!(!req.body.contains("name=\"icon\""));
        assert!(!req.body.contains("name=\"preview\""));
    }
}

#[tokio::test]
async fn asset_metadata_update_validates_local_input_before_sending() {
    let server = Server::success();
    for out in [
        cmd::asset::update(&server.ctx(), 123, None, None, None, None, None, &[]).await,
        cmd::asset::update(&server.ctx(), 0, None, None, Some("N"), None, None, &[]).await,
        cmd::asset::update(
            &server.ctx(),
            123,
            None,
            Some("Model"),
            Some("N"),
            None,
            None,
            &[],
        )
        .await,
        cmd::asset::update(
            &server.ctx(),
            123,
            None,
            None,
            None,
            None,
            Some("icon.png"),
            &[],
        )
        .await,
        cmd::asset::update(
            &server.ctx(),
            123,
            None,
            None,
            None,
            None,
            None,
            &["assets/0".into()],
        )
        .await,
    ] {
        assert!(!out.ok, "{out:?}");
        assert!(!out.hints.is_empty());
    }
    assert!(server.captured().is_empty());
}

#[tokio::test]
async fn asset_metadata_update_rejects_missing_or_wrong_receipts_without_resending() {
    for body in [
        json!({}),
        json!({"description": 7}),
        json!({"path": "assets/999"}),
        json!({"path": "assets/123", "assetId": "999"}),
        json!({"path": "assets/123", "error": {"code": 3, "message": "rejected"}}),
        json!({"path": "assets/123", "description": 7}),
    ] {
        let server = Server::new(move |_| Reply::Json(200, body.clone()));
        let out =
            cmd::asset::update(&server.ctx(), 123, None, None, None, Some("New"), None, &[]).await;
        assert!(!out.ok, "{out:?}");
        assert_eq!(out.error.unwrap().code, "invalid_asset_response");
        assert_eq!(server.captured().len(), 1);
    }
    let server = Server::new(|_| Reply::Json(503, json!({"message": "uncertain fixture"})));
    let out =
        cmd::asset::update(&server.ctx(), 123, None, None, None, Some("New"), None, &[]).await;
    assert_eq!(out.error.unwrap().code, "mutation_outcome_unknown");
    assert_eq!(server.captured().len(), 1);
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

fn part_present(body: &str, name: &str) -> bool {
    body.contains(&format!("name=\"{name}\""))
}

#[tokio::test]
async fn place_publish_sets_media_type_and_rejects_other_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("fixture.rbxl");
    std::fs::write(&binary, "rbxl-bytes").unwrap();
    let server = Server::new(|_| Reply::Json(200, json!({"versionNumber": 3})));
    let out = cmd::place::publish(&server.ctx(), 123, 456, &binary, true).await;
    assert!(out.ok, "{out:?}");
    let request = &server.captured()[0];
    assert_eq!(
        request.header("content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(request.body, "rbxl-bytes");

    let other = dir.path().join("fixture.txt");
    std::fs::write(&other, "nope").unwrap();
    let server = Server::success();
    let out = cmd::place::publish(&server.ctx(), 123, 456, &other, true).await;
    assert_eq!(out.error.unwrap().code, "unsupported_place_file");
    assert!(server.captured().is_empty());
}

#[tokio::test]
async fn oversized_place_files_fail_locally_before_any_request() {
    const LIMIT: u64 = crate::cmd::place::PLACE_PUBLISH_LIMIT_BYTES;
    let dir = tempfile::tempdir().unwrap();

    // Exactly at the documented limit stays eligible in both modes.
    let at_limit = dir.path().join("at-limit.rbxl");
    std::fs::File::create(&at_limit)
        .unwrap()
        .set_len(LIMIT)
        .unwrap();
    for published in [false, true] {
        let server = Server::new(|_| Reply::Json(200, json!({"versionNumber": 7})));
        let out = cmd::place::publish(&server.ctx(), 123, 456, &at_limit, published).await;
        assert!(out.ok, "{published}: {out:?}");
        let requests = server.captured();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].body.len() as u64, LIMIT);
    }

    // One byte over the limit never reaches the network, in either mode and
    // for both documented extensions.
    for published in [false, true] {
        for extension in ["rbxl", "rbxlx"] {
            let oversized = dir
                .path()
                .join(format!("oversized-{published}.{extension}"));
            std::fs::File::create(&oversized)
                .unwrap()
                .set_len(LIMIT + 1)
                .unwrap();
            let server = Server::success();
            let out = cmd::place::publish(&server.ctx(), 123, 456, &oversized, published).await;
            assert!(!out.ok, "{extension} {published}: {out:?}");
            assert!(server.captured().is_empty());
            let error = out.error.unwrap();
            assert_eq!(error.code, "place_file_too_large");
            assert!(!out.hints.is_empty());
            let detail = error.detail.unwrap();
            assert!(detail.contains(&(LIMIT + 1).to_string()), "{detail}");
            assert!(detail.contains(&LIMIT.to_string()), "{detail}");
        }
    }
}

#[tokio::test]
async fn place_update_sends_only_the_supplied_fields() {
    let reply = |_| {
        Reply::Json(
            200,
            json!({
                "path": "universes/123/places/456",
                "displayName": "Example Place",
                "description": "From a file",
                "serverSize": 20,
                "root": true
            }),
        )
    };

    // One field: the mask carries exactly that field.
    let single = Server::new(reply);
    let out = cmd::place::update(&single.ctx(), 123, 456, Some("Example Place"), None, None).await;
    assert!(out.ok, "{out:?}");
    let requests = single.captured();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "PATCH");
    assert_eq!(
        requests[0].target,
        "/cloud/v2/universes/123/places/456?updateMask=displayName"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&requests[0].body).unwrap(),
        json!({"displayName": "Example Place"})
    );

    // Several fields, including a description read from a file on the CLI side.
    let many = Server::new(reply);
    let out = cmd::place::update(
        &many.ctx(),
        123,
        456,
        Some("Example Place"),
        Some("From a file"),
        Some(20),
    )
    .await;
    assert!(out.ok, "{out:?}");
    let request = &many.captured()[0];
    // reqwest percent-encodes the comma separators; the decoded value is the
    // documented field mask.
    assert_eq!(
        request.target,
        "/cloud/v2/universes/123/places/456?updateMask=displayName%2Cdescription%2CserverSize"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&request.body).unwrap(),
        json!({"displayName": "Example Place", "description": "From a file", "serverSize": 20})
    );

    // A response that does not report the new value is not a success.
    let mismatch = Server::new(|_| {
        Reply::Json(
            200,
            json!({"path": "universes/123/places/456", "displayName": "Old name"}),
        )
    });
    let out = cmd::place::update(&mismatch.ctx(), 123, 456, Some("Example Place"), None, None).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(mismatch.captured().len(), 1);
    assert_eq!(out.error.unwrap().code, "place_response_mismatch");

    // A non-object success body is rejected too.
    let malformed = Server::new(|_| Reply::Json(200, json!([])));
    let out =
        cmd::place::update(&malformed.ctx(), 123, 456, Some("Example Place"), None, None).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(out.error.unwrap().code, "place_response_invalid");

    // Missing scope: the failure names universe.place:write.
    let forbidden = Server::new(|_| Reply::Json(403, json!({"message": "forbidden"})));
    let out =
        cmd::place::update(&forbidden.ctx(), 123, 456, Some("Example Place"), None, None).await;
    assert!(!out.ok, "{out:?}");
    assert!(
        out.hints
            .iter()
            .any(|hint| hint.contains("universe.place:write")),
        "{out:?}"
    );
}

#[tokio::test]
async fn place_update_never_looks_clearable_or_empty() {
    let server = Server::success();
    let ctx = server.ctx();
    // No field flags: fail before any cloud call.
    let out = cmd::place::update(&ctx, 123, 456, None, None, None).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(out.error.unwrap().code, "invalid_value");
    // Blank and zero values are rejected rather than sent.
    for (name, description, size) in [(Some("  "), None, None), (None, None, Some(0))] {
        let out = cmd::place::update(&ctx, 123, 456, name, description, size).await;
        assert!(!out.ok, "{name:?} {size:?}: {out:?}");
        assert_eq!(out.error.unwrap().code, "invalid_value");
    }
    assert!(server.captured().is_empty());

    // The dry-run plan needs no credentials and sends nothing.
    let plan = cmd::place::preview_update(123, 456, Some("Example Place"), Some("text"), Some(20));
    assert!(plan.ok, "{plan:?}");
    assert_eq!(plan.command, "place update (dry-run)");
    let data = plan.data.unwrap();
    assert_eq!(data["updateMask"], "displayName,description,serverSize");
    assert_eq!(data["dryRun"], true);
    assert_eq!(data["body"]["serverSize"], 20);
    assert!(server.captured().is_empty());
    assert!(!cmd::place::preview_update(123, 456, None, None, None).ok);
}

#[tokio::test]
async fn declared_empty_posts_send_no_json_object() {
    let server = Server::success();
    let ctx = server.ctx();
    assert!(
        cmd::config::restore(&ctx, 123, "InExperienceConfig", "rev-1")
            .await
            .ok
    );
    assert!(cmd::asset::archive(&ctx, 456).await.ok);
    assert!(cmd::server::restart(&ctx, 123, false).await.ok);
    assert!(
        cmd::config::publish(
            &ctx,
            123,
            "InExperienceConfig",
            Some("hash-1"),
            Some("ship"),
            Some("Immediate"),
        )
        .await
        .ok
    );
    let requests = server.captured();
    assert_eq!(requests[0].body, "");
    assert_ne!(requests[0].header("content-type"), Some("application/json"));
    assert_eq!(requests[1].body, "");
    assert_ne!(requests[1].header("content-type"), Some("application/json"));
    assert_eq!(requests[2].body, "{}");
    assert!(
        requests[2]
            .header("content-type")
            .unwrap()
            .starts_with("application/json")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&requests[3].body).unwrap(),
        json!({"draftHash": "hash-1", "message": "ship", "deploymentStrategy": "Immediate"})
    );
    let rejected = cmd::config::publish(
        &ctx,
        123,
        "InExperienceConfig",
        Some("hash-1"),
        None,
        Some("Invalid"),
    )
    .await;
    assert_eq!(rejected.error.unwrap().code, "invalid_deployment_strategy");
    assert_eq!(server.captured().len(), 4);
}

#[tokio::test]
async fn gamepass_price_does_not_imply_sale_on_update() {
    let server = Server::new(|_| Reply::Raw(204, String::new()));
    let ctx = server.ctx();
    assert!(
        cmd::monetization::gamepass_update(&ctx, 123, 7, None, None, Some(100), None, None)
            .await
            .ok
    );
    assert!(
        cmd::monetization::gamepass_update(&ctx, 123, 7, None, None, None, Some(false), None)
            .await
            .ok
    );
    assert!(
        cmd::monetization::gamepass_update(&ctx, 123, 7, None, None, Some(50), Some(true), None)
            .await
            .ok
    );
    let created = Server::new(|_| Reply::Json(200, json!({"gamePassId": 9})));
    assert!(
        cmd::monetization::gamepass_create(&created.ctx(), 123, "VIP", None, Some(499), None)
            .await
            .ok
    );
    let updates = server.captured();
    assert!(part_present(&updates[0].body, "price"));
    assert!(!part_present(&updates[0].body, "isForSale"));
    assert!(part_present(&updates[1].body, "isForSale"));
    assert!(!part_present(&updates[1].body, "price"));
    assert!(updates[1].body.contains("false"));
    assert!(part_present(&updates[2].body, "price"));
    assert!(part_present(&updates[2].body, "isForSale"));
    let create = &created.captured()[0];
    assert!(part_present(&create.body, "price"));
    assert!(part_present(&create.body, "isForSale"));
    assert!(create.body.contains("true"));
}

#[tokio::test]
async fn publish_message_preserves_the_exact_string() {
    let server = Server::success();
    let ctx = server.ctx();
    for message in ["hello", "\"hello\"", "{\"event\":\"restart\"}", "{}"] {
        assert!(
            cmd::universe::publish_message(&ctx, 123, "status", message)
                .await
                .ok,
            "{message}"
        );
    }
    let bodies: Vec<Value> = server
        .captured()
        .iter()
        .map(|request| serde_json::from_str(&request.body).unwrap())
        .collect();
    assert_eq!(
        bodies,
        vec![
            json!({"message": "hello"}),
            json!({"message": "hello"}),
            json!({"message": "{\"event\":\"restart\"}"}),
            json!({"message": "{}"}),
        ]
    );
    let huge = "x".repeat(1025);
    let idle = Server::success();
    let out = cmd::universe::publish_message(&idle.ctx(), 123, "status", &huge).await;
    assert_eq!(out.error.unwrap().code, "message_too_large");
    assert!(idle.captured().is_empty());
}

#[tokio::test]
async fn permission_subjects_follow_the_empty_id_rule() {
    let server = Server::new(|_| Reply::Json(200, json!({"successAssetIds": [111], "errors": []})));
    let ctx = server.ctx();
    for (subject, id) in [
        ("all", None),
        ("user", Some(4u64)),
        ("group", Some(5)),
        ("group-roleset", Some(6)),
        ("universe", Some(7)),
    ] {
        let out = cmd::permissions::grant(&ctx, &[111], subject, id, "use").await;
        assert!(out.ok, "{subject} {out:?}");
    }
    let bodies: Vec<Value> = server
        .captured()
        .iter()
        .map(|request| serde_json::from_str(&request.body).unwrap())
        .collect();
    assert_eq!(bodies[0]["subjectType"], "All");
    assert_eq!(bodies[0]["subjectId"], "");
    assert_eq!(bodies[1]["subjectId"], "4");
    assert_eq!(bodies[2]["subjectType"], "Group");
    assert_eq!(bodies[3]["subjectType"], "GroupRoleset");
    assert_eq!(bodies[4]["subjectType"], "Universe");
    let idle = Server::success();
    let forbidden = cmd::permissions::grant(&idle.ctx(), &[111], "all", Some(4), "use").await;
    assert_eq!(forbidden.error.unwrap().code, "subject_id_forbidden");
    let missing = cmd::permissions::grant(&idle.ctx(), &[111], "user", None, "use").await;
    assert_eq!(missing.error.unwrap().code, "subject_id_required");
    assert!(idle.captured().is_empty());
}

#[tokio::test]
async fn list_pages_forward_provider_tokens_with_exact_query_names() {
    let server = Server::new(|_| {
        Reply::Json(
            200,
            json!({"nextPageToken": "next", "nextCursor": "cur", "previousPageToken": "prev"}),
        )
    });
    let ctx = server.ctx();
    assert!(cmd::thumbnail::list(&ctx, 123, Some("cur")).await.ok);
    assert!(cmd::server::list(&ctx, 123, 456, 7, Some("page")).await.ok);
    assert!(cmd::place::history(&ctx, 456, 10, Some("cur")).await.ok);
    assert!(
        cmd::datastore::list(&ctx, 123, "store", 10, Some("page"))
            .await
            .ok
    );
    assert!(
        cmd::ordered_datastore::list(&ctx, 123, "store", "global", false, 10, Some("page"))
            .await
            .ok
    );
    assert!(
        cmd::monetization::gamepass_list(&ctx, 123, Some("page"))
            .await
            .ok
    );
    assert!(
        cmd::monetization::devproduct_list(&ctx, 123, Some("page"))
            .await
            .ok
    );
    assert!(cmd::event::list(&ctx, 123, 10, Some("page"), true).await.ok);
    assert!(
        cmd::server::logs(&ctx, 123, 456, 7, "job", 100, false, Some("page"))
            .await
            .ok
    );
    let targets: Vec<_> = server
        .captured()
        .into_iter()
        .map(|request| request.target)
        .collect();
    assert!(targets[0].contains("nextCursor=cur"));
    assert!(targets[1].contains("PageToken=page"));
    assert!(targets[2].contains("cursor=cur"));
    assert!(targets[2].contains("pageSize=10"));
    assert!(targets[3].contains("pageToken=page"));
    assert!(targets[3].contains("maxPageSize=10"));
    assert!(targets[4].contains("page_token=page"));
    assert!(targets[4].contains("max_page_size=10"));
    assert!(targets[5].contains("pageToken=page"));
    assert!(targets[6].contains("pageToken=page"));
    assert!(targets[7].contains("pageToken=page"));
    assert!(targets[7].contains("reverse=true"));
    assert!(targets[8].contains("MaxPageSize=100"));
    assert!(targets[8].contains("PageToken=page"));
}

#[tokio::test]
async fn server_logs_follow_pages_and_reject_bad_limits() {
    let server = Server::new(|count| {
        if count == 1 {
            Reply::Json(
                200,
                json!({"gameServerLogs": [{"message": "a"}], "nextPageToken": "p2"}),
            )
        } else {
            Reply::Json(200, json!({"gameServerLogs": [{"message": "b"}]}))
        }
    });
    let out = cmd::server::logs(&server.ctx(), 123, 456, 7, "job", 0, false, None).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.data.unwrap()["entries"].as_array().unwrap().len(), 2);
    let requests = server.captured();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].target.contains("MaxPageSize=100"));
    assert!(requests[1].target.contains("PageToken=p2"));

    let looping = Server::new(|_| {
        Reply::Json(
            200,
            json!({"gameServerLogs": [{"message": "a"}], "nextPageToken": "again"}),
        )
    });
    let out = cmd::server::logs(&looping.ctx(), 123, 456, 7, "job", 0, true, None).await;
    assert_eq!(out.error.unwrap().code, "pagination_loop");
    assert_eq!(looping.captured().len(), 2);

    let idle = Server::success();
    let out = cmd::server::logs(&idle.ctx(), 123, 456, 7, "job", 101, false, None).await;
    assert_eq!(out.error.unwrap().code, "invalid_limit");
    assert!(idle.captured().is_empty());
}

#[tokio::test]
async fn datastore_value_updates_preserve_metadata() {
    let missing = Server::new(|_| Reply::Json(404, json!({"message": "missing"})));
    let out = cmd::datastore::set(
        &missing.ctx(),
        123,
        "store",
        "player",
        None,
        "1",
        false,
        cmd::datastore::ValueUpdate::default(),
    )
    .await;
    assert_eq!(out.error.unwrap().code, "entry_not_found");
    assert_eq!(missing.captured().len(), 1);
    assert_eq!(missing.captured()[0].method, "GET");

    let created = Server::new(|count| {
        if count == 1 {
            Reply::Json(404, json!({"message": "missing"}))
        } else {
            Reply::Json(200, json!({"value": 1}))
        }
    });
    let out = cmd::datastore::set(
        &created.ctx(),
        123,
        "store",
        "player",
        Some("global"),
        "1",
        true,
        cmd::datastore::ValueUpdate::default(),
    )
    .await;
    assert!(out.ok, "{out:?}");
    let patch = &created.captured()[1];
    assert_eq!(patch.method, "PATCH");
    assert!(patch.target.contains("/scopes/global/"));
    assert!(patch.target.contains("allowMissing=true"));
    assert_eq!(
        serde_json::from_str::<Value>(&patch.body).unwrap(),
        json!({"value": 1})
    );

    let existing = Server::new(|_| {
        Reply::Json(
            200,
            json!({"value": 1, "attributes": {"rank": 1}, "users": ["users/7"], "etag": "e1"}),
        )
    });
    let out = cmd::datastore::set(
        &existing.ctx(),
        123,
        "store",
        "player",
        None,
        "2",
        false,
        cmd::datastore::ValueUpdate {
            attributes_json: Some(r#"{"rank":2}"#),
            users_json: Some(r#"["users/9"]"#),
            ..cmd::datastore::ValueUpdate::default()
        },
    )
    .await;
    assert!(out.ok, "{out:?}");
    assert_eq!(
        serde_json::from_str::<Value>(&existing.captured()[1].body).unwrap(),
        json!({"value": 2, "attributes": {"rank": 2}, "users": ["users/9"], "etag": "e1"})
    );

    let cleared = Server::new(|_| {
        Reply::Json(
            200,
            json!({"value": 1, "attributes": {"rank": 1}, "users": ["users/7"], "etag": "e1"}),
        )
    });
    let out = cmd::datastore::set(
        &cleared.ctx(),
        123,
        "store",
        "player",
        None,
        "3",
        false,
        cmd::datastore::ValueUpdate {
            clear_attributes: true,
            clear_users: true,
            ..cmd::datastore::ValueUpdate::default()
        },
    )
    .await;
    assert!(out.ok, "{out:?}");
    assert_eq!(
        serde_json::from_str::<Value>(&cleared.captured()[1].body).unwrap(),
        json!({"value": 3, "attributes": {}, "users": [], "etag": "e1"})
    );

    let bare = Server::new(|_| Reply::Json(200, json!({"value": 1})));
    let out = cmd::datastore::set(
        &bare.ctx(),
        123,
        "store",
        "player",
        None,
        "4",
        false,
        cmd::datastore::ValueUpdate::default(),
    )
    .await;
    assert_eq!(out.error.unwrap().code, "etag_missing");
    assert_eq!(bare.captured().len(), 1);

    let conflict = Server::success();
    let out = cmd::datastore::set(
        &conflict.ctx(),
        123,
        "store",
        "player",
        None,
        "5",
        false,
        cmd::datastore::ValueUpdate {
            attributes_json: Some("{}"),
            clear_attributes: true,
            ..cmd::datastore::ValueUpdate::default()
        },
    )
    .await;
    assert_eq!(out.error.unwrap().code, "metadata_conflict");
    assert!(conflict.captured().is_empty());

    let denied = Server::new(|_| Reply::Json(403, json!({"message": "permission denied"})));
    let out = cmd::datastore::set(
        &denied.ctx(),
        123,
        "store",
        "player",
        None,
        "6",
        false,
        cmd::datastore::ValueUpdate::default(),
    )
    .await;
    let error = out.error.unwrap();
    assert_eq!(error.code, "datastore_read_forbidden");
    assert!(
        error
            .detail
            .unwrap()
            .contains("universe-datastores.objects:read")
    );
    assert_eq!(denied.captured().len(), 1);
    assert_eq!(denied.captured()[0].method, "GET");
}

#[tokio::test]
async fn standalone_asset_files_are_rejected_before_upload() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.png");
    let empty = dir.path().join("empty.fbx");
    std::fs::write(&empty, b"").unwrap();
    let over = dir.path().join("over.png");
    std::fs::File::create(&over)
        .unwrap()
        .set_len(cmd::asset::MAX_ASSET_BYTES + 1)
        .unwrap();
    let boundary = dir.path().join("boundary.png");
    std::fs::File::create(&boundary)
        .unwrap()
        .set_len(cmd::asset::MAX_ASSET_BYTES)
        .unwrap();
    let server = Server::success();
    let ctx = server.ctx();
    for file in [&missing, &empty, &over] {
        let created = cmd::asset::create(&ctx, file, "N", "", Some(1), None, None).await;
        assert!(!created.ok, "{created:?}");
        let updated = cmd::asset::update(&ctx, 9, Some(file), None, None, None, None, &[]).await;
        assert!(!updated.ok, "{updated:?}");
    }
    let directory = dir.path().join("not-a-file.png");
    std::fs::create_dir(&directory).unwrap();
    assert_eq!(
        cmd::asset::preview_create(&missing, "N", "", Some(1), None, None)
            .await
            .error
            .unwrap()
            .code,
        "file_unreadable"
    );
    assert_eq!(
        cmd::asset::preview_create(&directory, "N", "", Some(1), None, None)
            .await
            .error
            .unwrap()
            .code,
        "file_unreadable"
    );
    assert_eq!(
        cmd::asset::preview_create(&empty, "N", "", Some(1), None, None)
            .await
            .error
            .unwrap()
            .code,
        "file_size_invalid"
    );
    assert_eq!(
        cmd::asset::preview_update(Some(&over), 9, None, None, None, None, &[])
            .await
            .error
            .unwrap()
            .code,
        "file_size_invalid"
    );
    let preview = cmd::asset::preview_create(&boundary, "N", "", Some(1), None, None).await;
    assert!(preview.ok, "{preview:?}");
    assert_eq!(preview.data.unwrap()["bytes"], cmd::asset::MAX_ASSET_BYTES);
    assert!(server.captured().is_empty());
}

fn metrics_query() -> &'static str {
    r#"{"metric":"DailyActiveUsers","granularity":"OneDay","startTime":"2026-10-01T00:00:00Z","endTime":"2026-10-08T00:00:00Z"}"#
}

fn operation_id_from_hint(hint: &str) -> &str {
    hint.split("--operation-id ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .trim_matches('`')
}

#[tokio::test]
async fn datastore_403_includes_its_catalog_permission() {
    let server = Server::new(|_| Reply::Json(403, json!({"message": "forbidden"})));
    let out = cmd::datastore::get(&server.ctx(), 123, "store", "entry", None).await;
    assert!(!out.ok);
    assert_eq!(out.error.as_ref().unwrap().status, Some(403));
    assert!(
        out.hints
            .iter()
            .any(|hint| hint.contains("universe-datastores.objects:read"))
    );
    assert!(
        out.hints
            .iter()
            .any(|hint| hint.contains("rbxoc api show datastore.get"))
    );
}

#[tokio::test]
async fn event_get_403_names_its_dual_auth_scope() {
    let server = Server::new(|_| Reply::Json(403, json!({"message": "forbidden"})));
    let out = cmd::event::get(&server.ctx(), 456).await;
    assert!(
        out.hints
            .iter()
            .any(|hint| hint.contains("universe.event:read"))
    );
    assert!(
        out.hints
            .iter()
            .any(|hint| hint.contains("rbxoc api show event.get"))
    );
}

#[tokio::test]
async fn datastore_set_403_names_the_mutation_scope() {
    let server = Server::new(|count| {
        if count == 1 {
            Reply::Json(
                200,
                json!({"etag": "etag-1", "value": 1, "attributes": {}, "users": []}),
            )
        } else {
            Reply::Json(403, json!({"message": "forbidden"}))
        }
    });
    let out = cmd::datastore::set(
        &server.ctx(),
        123,
        "store",
        "entry",
        None,
        "1",
        false,
        cmd::datastore::ValueUpdate::default(),
    )
    .await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(server.captured().len(), 2);
    assert_eq!(server.captured()[1].method, "PATCH");
    assert!(
        out.hints
            .iter()
            .any(|hint| hint.contains("universe-datastores.objects:update"))
    );
    assert!(
        out.hints
            .iter()
            .any(|hint| hint.contains("rbxoc api show datastore.set"))
    );
}

#[tokio::test]
async fn universe_get_without_scopes_keeps_generic_hints() {
    let server = Server::new(|_| Reply::Json(403, json!({"message": "forbidden"})));
    let out = cmd::universe::get(&server.ctx(), 123).await;
    assert!(!out.ok);
    assert!(!out.hints.is_empty());
    assert!(
        !out.hints
            .iter()
            .any(|hint| hint.contains("Creator Dashboard → API keys → Permissions:"))
    );
}

#[tokio::test]
async fn place_update_checks_server_size_before_success() {
    let matched = Server::new(|_| {
        Reply::Json(
            200,
            json!({"path": "universes/123/places/456", "displayName": "Fixture", "serverSize": 20}),
        )
    });
    let out = cmd::place::update(&matched.ctx(), 123, 456, None, None, Some(20)).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(matched.captured().len(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&matched.captured()[0].body).unwrap(),
        json!({"serverSize": 20})
    );

    let mismatch = Server::new(|_| {
        Reply::Json(
            200,
            json!({"path": "universes/123/places/456", "displayName": "Fixture", "serverSize": 10}),
        )
    });
    let out = cmd::place::update(&mismatch.ctx(), 123, 456, None, None, Some(20)).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(out.error.as_ref().unwrap().code, "place_response_mismatch");
    assert!(!out.hints.is_empty());
    assert_eq!(mismatch.captured().len(), 1);

    for body in [
        json!({"path": "universes/123/places/456", "displayName": "Fixture"}),
        json!({"serverSize": "20"}),
        json!({"serverSize": 0}),
    ] {
        let server = Server::new({
            let body = body.clone();
            move |_| Reply::Json(200, body.clone())
        });
        let out = cmd::place::update(&server.ctx(), 123, 456, None, None, Some(20)).await;
        assert!(!out.ok, "{body} {out:?}");
        assert_eq!(out.error.unwrap().code, "place_response_mismatch");
        assert_eq!(server.captured().len(), 1);
    }

    let combined = Server::new(|_| {
        Reply::Json(
            200,
            json!({
                "displayName": "Example Place",
                "description": "From a file",
                "serverSize": 20
            }),
        )
    });
    let out = cmd::place::update(
        &combined.ctx(),
        123,
        456,
        Some("Example Place"),
        Some("From a file"),
        Some(20),
    )
    .await;
    assert!(out.ok, "{out:?}");
    let preview = cmd::place::preview_update(123, 456, None, None, Some(20));
    assert!(preview.ok, "{preview:?}");
    assert_eq!(preview.data.unwrap()["dryRun"], true);
    assert_eq!(combined.captured().len(), 1);
}

#[tokio::test]
async fn analytics_operation_paths_round_trip() {
    let server = Server::new(|_| {
        Reply::Json(
            202,
            json!({"path": "v1/universes/123/operations/metrics/abc123", "done": false}),
        )
    });
    let out = cmd::analytics::metrics(&server.ctx(), 123, metrics_query()).await;
    assert!(out.ok, "{out:?}");
    let hint = out
        .hints
        .iter()
        .find(|hint| hint.contains("--operation-id"))
        .expect("poll hint");
    let id = operation_id_from_hint(hint);
    assert_eq!(id, "abc123");
    let _polled = cmd::analytics::operation(&server.ctx(), 123, "metrics", id).await;
    let requests = server.captured();
    assert_eq!(
        requests.last().unwrap().target,
        "/analytics-query-api/v1/universes/123/operations/metrics/abc123"
    );
    assert_eq!(requests.last().unwrap().method, "GET");

    let dimensions = Server::success();
    let out = cmd::analytics::operation(
        &dimensions.ctx(),
        123,
        "dimensions",
        "v1/universes/123/operations/dimension-values/xyz",
    )
    .await;
    assert!(out.ok, "{out:?}");
    assert_eq!(
        dimensions.captured()[0].target,
        "/analytics-query-api/v1/universes/123/operations/dimension-values/xyz"
    );

    let mismatch = Server::success();
    let out = cmd::analytics::operation(
        &mismatch.ctx(),
        123,
        "metrics",
        "v1/universes/999/operations/metrics/abc123",
    )
    .await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(out.error.unwrap().code, "invalid_operation_path");
    assert!(mismatch.captured().is_empty());
    assert!(!out.hints.iter().any(|hint| hint.contains("--operation-id")));

    let malformed = Server::new(|_| {
        Reply::Json(
            202,
            json!({"path": "v1/universes/999/operations/metrics/abc123", "done": false}),
        )
    });
    let out = cmd::analytics::metrics(&malformed.ctx(), 123, metrics_query()).await;
    assert!(out.ok, "{out:?}");
    assert!(!out.hints.iter().any(|hint| hint.contains("--operation-id")));
    assert!(out.hints.iter().any(|hint| hint.contains("Reconcile")));
    assert_eq!(malformed.captured().len(), 1);
}

#[tokio::test]
async fn badge_create_sends_the_full_paid_form_or_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let icon = dir.path().join("badge.png");
    std::fs::write(&icon, b"png-bytes").unwrap();
    let server = Server::success();
    let mut request = paid_badge("Test", "Description", Some("user"));
    request.icon = Some(&icon);
    let out = cmd::badge::create(&server.ctx(), 123, request).await;
    assert!(out.ok, "{out:?}");
    let body = &server.captured()[0].body;
    assert!(body.contains("name=\"files\""), "{body}");
    assert!(body.contains("filename=\"badge.png\""), "{body}");
    assert!(body.contains("image/png"), "{body}");
    assert!(
        body.contains("name=\"expectedCost\"\r\n\r\n100\r\n"),
        "{body}"
    );
    assert!(body.contains("name=\"isActive\"\r\n\r\ntrue\r\n"), "{body}");

    let missing_cost = Server::success();
    let mut request = paid_badge("Test", "Description", Some("user"));
    request.expected_cost = None;
    let out = cmd::badge::create(&missing_cost.ctx(), 123, request).await;
    assert_eq!(out.error.unwrap().code, "expected_cost_missing");
    assert!(missing_cost.captured().is_empty());

    let missing_active = Server::success();
    let mut request = paid_badge("Test", "Description", Some("user"));
    request.active = None;
    let out = cmd::badge::create(&missing_active.ctx(), 123, request).await;
    assert_eq!(out.error.unwrap().code, "active_missing");
    assert!(missing_active.captured().is_empty());

    let text = dir.path().join("badge.txt");
    std::fs::write(&text, b"nope").unwrap();
    let rejected = Server::success();
    let mut request = paid_badge("Test", "Description", Some("user"));
    request.icon = Some(&text);
    let out = cmd::badge::create(&rejected.ctx(), 123, request).await;
    assert_eq!(out.error.unwrap().code, "invalid_badge_icon");
    assert!(rejected.captured().is_empty());

    let empty = dir.path().join("empty.png");
    std::fs::write(&empty, b"").unwrap();
    let rejected = Server::success();
    let mut request = paid_badge("Test", "Description", Some("user"));
    request.icon = Some(&empty);
    let out = cmd::badge::create(&rejected.ctx(), 123, request).await;
    assert_eq!(out.error.unwrap().code, "file_size_invalid");
    assert!(rejected.captured().is_empty());
}
