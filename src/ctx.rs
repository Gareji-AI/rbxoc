//! Shared command building blocks.
//!
//! Each subcommand takes a `Ctx` and returns an `Output`. Errors ride in the
//! `error` field of `Output` so the caller can decide the exit code.

use anyhow::Result;
use serde_json::Value;

use crate::auth::{self, Auth};
use crate::output::{ErrorBody, Output};

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
            // RequestBuilder is cloneable, which retries need.
            let this = req.try_clone().ok_or_else(|| ErrorBody {
                code: "request_not_retryable",
                message: "cannot rebuild the request (streaming body)".into(),
                status: None,
                detail: None,
            })?;
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
        })?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| ErrorBody {
            code: "invalid_json",
            message: format!("the response is not JSON: {e}"),
            status: None,
            detail: Some(text.chars().take(400).collect()),
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn failure_carries_hints_for_http_errors() {
        let out = failure(
            "test",
            ErrorBody {
                code: "http_403",
                message: "forbidden".into(),
                status: Some(403),
                detail: None,
            },
        );
        assert!(!out.ok);
        assert!(!out.hints.is_empty());
    }
}
