//! Shared command building blocks.
//!
//! Each subcommand takes a `Ctx` and returns an `Output`. Errors ride in the
//! `error` field of `Output` so the caller can decide the exit code.

use anyhow::Result;
use serde_json::Value;

use crate::auth::{self, Auth};
use crate::open_cloud_errors;
use crate::output::{ErrorBody, Output};

/// One named URL segment, encoded exactly once from its raw UTF-8 value.
/// Reject empty/dot-only segments: URL parsers normalize these structurally,
/// including percent-encoded dots. All other bytes outside RFC 3986 unreserved
/// characters are encoded; literal percent signs are not pre-encoded input.
pub fn path_segment(value: &str) -> Result<String, ErrorBody> {
    if matches!(value, "" | "." | "..") {
        return Err(ErrorBody {
            code: "invalid_path_segment",
            message: "Named URL segments must not be empty, '.' or '..'".into(),
            status: None,
            detail: None,
            refs: None,
        });
    }
    const ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');
    Ok(percent_encoding::utf8_percent_encode(value, ENCODE).to_string())
}

/// Execution context.
pub struct Ctx {
    pub client: reqwest::Client,
    pub auth: Auth,
    /// Automatic retry count for 429 / 5xx.
    pub retry: u32,
    pub api_host: String,
}

impl Ctx {
    pub fn new(auth: Auth, retry: u32) -> Result<Self> {
        Ok(Self {
            client: auth::http_client()?,
            auth,
            retry,
            api_host: auth::API_HOST.to_string(),
        })
    }

    /// Build an absolute URL. `path` starts with `/`.
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.api_host, path)
    }

    /// Attach the auth header. `Auth` decides the scheme.
    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            Auth::ApiKey(key) => req.header("x-api-key", key),
            Auth::OAuth(token) => req.bearer_auth(token),
        }
    }

    /// Send a request. 429 / 5xx retry with exponential backoff.
    pub async fn send(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response, ErrorBody> {
        let req = self.apply_auth(req);
        let mut attempt = 0u32;
        loop {
            // A multipart body is a stream and cannot be cloned, so retrying is
            // impossible for uploads. Send those exactly once instead of
            // failing before the request leaves the process.
            let this = match req.try_clone() {
                Some(ok) => ok,
                None => {
                    return req.send().await.map_err(|err| ErrorBody {
                        code: "network_error",
                        message: format!("cannot send the request: {err}"),
                        status: None,
                        detail: Some(
                            "This request streams a body, so it cannot be retried automatically"
                                .into(),
                        ),
                        refs: None,
                    });
                }
            };
            match this.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let retryable = status == 429 || status >= 500;
                    if retryable && attempt < self.retry {
                        let wait = backoff_ms(attempt, resp.headers());
                        attempt += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    // Connection failures also retry.
                    if attempt < self.retry {
                        attempt += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms(
                            attempt,
                            &reqwest::header::HeaderMap::new(),
                        )))
                        .await;
                        continue;
                    }
                    return Err(ErrorBody {
                        code: "network_error",
                        message: format!("cannot send the request: {err}"),
                        status: None,
                        detail: None,
                        refs: None,
                    });
                }
            }
        }
    }

    /// GET that expects JSON.
    pub async fn get_json(&self, path: &str) -> Result<Value, ErrorBody> {
        let resp = self.send(self.client.get(self.url(path))).await?;
        self.json_or_error(resp).await
    }

    /// GET with query values encoded independently of the route.
    pub async fn get_json_query(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Value, ErrorBody> {
        let resp = self
            .send(self.client.get(self.url(path)).query(query))
            .await?;
        self.json_or_error(resp).await
    }

    /// For deletes and other bodyless calls.
    pub async fn send_empty(&self, req: reqwest::RequestBuilder) -> Result<Value, ErrorBody> {
        let resp = self.send(req).await?;
        if !resp.status().is_success() {
            return Err(auth::error_from_response(resp).await);
        }
        let status = resp.status().as_u16();
        Ok(serde_json::json!({ "status": status }))
    }

    /// JSON on success, `ErrorBody` on failure.
    pub async fn json_or_error(&self, resp: reqwest::Response) -> Result<Value, ErrorBody> {
        if !resp.status().is_success() {
            return Err(auth::error_from_response(resp).await);
        }
        let text = resp.text().await.map_err(|e| ErrorBody {
            code: "read_error",
            message: format!("cannot read the response: {e}"),
            status: None,
            detail: None,

            refs: None,
        })?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| ErrorBody {
            code: "invalid_json",
            message: format!("the response is not JSON: {e}"),
            status: None,
            detail: Some(text.chars().take(400).collect()),

            refs: None,
        })
    }
}

/// Retry wait. Respects `Retry-After`.
fn backoff_ms(attempt: u32, headers: &reqwest::header::HeaderMap) -> u64 {
    if let Some(v) = headers.get("retry-after").and_then(|v| v.to_str().ok())
        && let Ok(secs) = v.trim().parse::<u64>()
    {
        return secs.saturating_mul(1000).min(60_000);
    }
    let base = 500u64;
    (base * 2u64.pow(attempt.min(5))).min(30_000)
}

/// Build a failure `Output` from an `ErrorBody`, including hints.
pub fn failure(command: &str, error: ErrorBody) -> Output {
    let mut hints = Vec::new();
    if let Some(status) = error.status {
        hints.extend(auth::hints_for_status(status));
    }
    if error.code == "auth_missing" {
        hints.push("Set RBXOC_API_KEY".into());
        hints.push("Run `rbxoc auth check` to inspect the credential state".into());
    }
    Output {
        ok: false,
        command: command.to_string(),
        summary: error.message.clone(),
        data: None,
        hints,
        error: Some(error),
    }
}

/// Failure output with catalog-backed permission hints for an endpoint.
pub fn failure_for_endpoint(
    command: &str,
    endpoint_id: &str,
    error: ErrorBody,
    auth: &Auth,
) -> Output {
    let error = open_cloud_errors::apply_endpoint_context(error, endpoint_id, auth.kind());
    let mut out = failure(command, error);
    let Some(err) = out.error.as_ref() else {
        return out;
    };
    if err.code == "feature_not_enrolled" {
        return out;
    }
    if let Some(status) = err.status {
        out.hints
            .extend(open_cloud_errors::permission_hints_for_endpoint(
                endpoint_id,
                status,
            ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_segments_preserve_request_structure() {
        for (raw, expected) in [
            ("/", "%2F"),
            ("?", "%3F"),
            ("#", "%23"),
            (" ", "%20"),
            ("%", "%25"),
            ("日本語", "%E6%97%A5%E6%9C%AC%E8%AA%9E"),
            ("a/../b?x=1#frag", "a%2F..%2Fb%3Fx%3D1%23frag"),
            ("name-._~", "name-._~"),
            ("%2F", "%252F"),
        ] {
            let segment = path_segment(raw).unwrap();
            assert_eq!(segment, expected);
            let url =
                reqwest::Url::parse(&format!("https://example.invalid/entries/{segment}/tail"))
                    .unwrap();
            assert_eq!(url.path(), format!("/entries/{expected}/tail"));
            assert_eq!(url.query(), None);
            assert_eq!(url.fragment(), None);
        }
        for raw in ["", ".", ".."] {
            assert_eq!(path_segment(raw).unwrap_err().code, "invalid_path_segment");
        }
    }

    #[test]
    fn backoff_grows_and_caps() {
        let empty = reqwest::header::HeaderMap::new();
        assert_eq!(backoff_ms(0, &empty), 500);
        assert_eq!(backoff_ms(1, &empty), 1000);
        assert!(backoff_ms(10, &empty) <= 30_000);
    }

    #[test]
    fn backoff_respects_retry_after() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("retry-after", "3".parse().unwrap());
        assert_eq!(backoff_ms(0, &h), 3000);
        // Clamped at the ceiling.
        h.insert("retry-after", "9999".parse().unwrap());
        assert_eq!(backoff_ms(0, &h), 60_000);
    }

    #[test]
    fn failure_for_endpoint_adds_permission_hints() {
        let out = failure_for_endpoint(
            "analytics alerts list",
            "analytics.alert.list",
            ErrorBody {
                code: "http_403",
                message: "forbidden".into(),
                status: Some(403),
                detail: None,
                refs: None,
            },
            &Auth::ApiKey("k".into()),
        );
        assert!(
            out.hints
                .iter()
                .any(|h| h.contains("universe.analytics.alert:read"))
        );
    }

    #[test]
    fn failure_for_endpoint_skips_hints_when_not_enrolled() {
        let out = failure_for_endpoint(
            "analytics alerts list",
            "analytics.alert.list",
            ErrorBody {
                code: "feature_not_enrolled",
                message: "not enrolled".into(),
                status: Some(403),
                detail: None,
                refs: None,
            },
            &Auth::ApiKey("k".into()),
        );
        assert!(!out.hints.iter().any(|h| h.contains("Creator Dashboard")));
    }

    #[test]
    fn failure_carries_hints_for_http_errors() {
        let out = failure(
            "test",
            ErrorBody {
                code: "http_403",
                message: "forbidden".into(),
                status: Some(403),
                detail: None,
                refs: None,
            },
        );
        assert!(!out.ok);
        assert!(!out.hints.is_empty());
    }
}
