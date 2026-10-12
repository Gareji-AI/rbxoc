//! コマンドの共通部品。
//!
//! 各サブコマンドは `Ctx` を受け取り、`Output` を返す。エラーは `Output` の
//! `error` に載せて返すので、呼び出し側で終了コードを決められる。

use anyhow::Result;
use serde_json::Value;

use crate::auth::{self, Auth};
use crate::output::{ErrorBody, Output};

/// 実行コンテキスト。
pub struct Ctx {
    pub client: reqwest::Client,
    pub auth: Auth,
    /// 429 / 5xx で自動再試行する回数。
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

    /// 絶対 URL を組み立てる。`path` は `/` 始まり。
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.api_host, path)
    }

    /// 認証ヘッダを付ける。方式は `Auth` が決める。
    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            Auth::ApiKey(key) => req.header("x-api-key", key),
            Auth::OAuth(token) => req.bearer_auth(token),
        }
    }

    /// リクエストを送る。429 / 5xx は指数バックオフで再試行する。
    pub async fn send(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response, ErrorBody> {
        let req = self.apply_auth(req);
        let mut attempt = 0u32;
        loop {
            // RequestBuilder は clone できる。再試行に必要。
            let this = req.try_clone().ok_or_else(|| ErrorBody {
                code: "request_not_retryable",
                message: "リクエストを再構成できません（ストリーム本文）".into(),
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
                    // 接続系の失敗も再試行する。
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
                        message: format!("リクエストを送れません: {err}"),
                        status: None,
                        detail: None,
                    });
                }
            }
        }
    }

    /// JSON を期待する GET。
    pub async fn get_json(&self, path: &str) -> Result<Value, ErrorBody> {
        let resp = self.send(self.client.get(self.url(path))).await?;
        self.json_or_error(resp).await
    }

    /// 本文なしの DELETE など。
    pub async fn send_empty(&self, req: reqwest::RequestBuilder) -> Result<Value, ErrorBody> {
        let resp = self.send(req).await?;
        if !resp.status().is_success() {
            return Err(auth::error_from_response(resp).await);
        }
        let status = resp.status().as_u16();
        Ok(serde_json::json!({ "status": status }))
    }

    /// 成功なら JSON、失敗なら `ErrorBody`。
    pub async fn json_or_error(&self, resp: reqwest::Response) -> Result<Value, ErrorBody> {
        if !resp.status().is_success() {
            return Err(auth::error_from_response(resp).await);
        }
        let text = resp.text().await.map_err(|e| ErrorBody {
            code: "read_error",
            message: format!("応答を読めません: {e}"),
            status: None,
            detail: None,
        })?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| ErrorBody {
            code: "invalid_json",
            message: format!("応答が JSON ではありません: {e}"),
            status: None,
            detail: Some(text.chars().take(400).collect()),
        })
    }
}

/// 再試行の待ち時間。`Retry-After` を尊重する。
fn backoff_ms(attempt: u32, headers: &reqwest::header::HeaderMap) -> u64 {
    if let Some(v) = headers.get("retry-after").and_then(|v| v.to_str().ok()) {
        if let Ok(secs) = v.trim().parse::<u64>() {
            return secs.saturating_mul(1000).min(60_000);
        }
    }
    let base = 500u64;
    (base * 2u64.pow(attempt.min(5))).min(30_000)
}

/// `ErrorBody` から失敗の `Output` を作る。hint も一緒に載せる。
pub fn failure(command: &str, error: ErrorBody) -> Output {
    let mut hints = Vec::new();
    if let Some(status) = error.status {
        hints.extend(auth::hints_for_status(status));
    }
    if error.code == "auth_missing" {
        hints.push("RBXOC_API_KEY を設定してください".into());
        hints.push("`rbxoc auth check` で状態を確認できます".into());
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
        // 上限で切る。
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
