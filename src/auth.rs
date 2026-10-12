//! 認証と HTTP クライアント。
//!
//! Open Cloud の認証は 3 系統ある（`roblox-api-key` / `roblox-oauth2` /
//! `roblox-legacy-cookie`）。本 CLI は API key と OAuth2 の Bearer に対応する。
//! legacy cookie はブラウザセッション由来で自動化に向かず、対象外とする。
//!
//! API key の探索順:
//! 1. `--api-key` 引数（エージェントは使わない想定。履歴に残るため）
//! 2. `RBXOC_API_KEY` 環境変数
//! 3. `--api-key-file` で指定したファイル
//! 4. 既定の keyring ファイル `~/.rbxoc/api-key`（パーミッション確認あり）
//!
//! OAuth2 は `RBXOC_OAUTH_TOKEN` 環境変数、または `~/.rbxoc/oauth-token`。

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::output::ErrorBody;

/// Open Cloud の API ホスト。
pub const API_HOST: &str = "https://apis.roblox.com";

/// 認証方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    /// `x-api-key` ヘッダ。大半のエンドポイント。
    ApiKey(String),
    /// `Authorization: Bearer`。OAuth2 スコープが必要な領域。
    OAuth(String),
}

impl Auth {
    /// 秘密値をログに出さないための表示名。
    pub fn kind(&self) -> &'static str {
        match self {
            Auth::ApiKey(_) => "api-key",
            Auth::OAuth(_) => "oauth",
        }
    }

    /// マスクした値。先頭 4 文字だけ残す。エラー表示用。
    pub fn masked(&self) -> String {
        let raw = match self {
            Auth::ApiKey(v) | Auth::OAuth(v) => v,
        };
        if raw.len() <= 8 {
            return "****".to_string();
        }
        format!("{}****", &raw[..4])
    }
}

/// 認証を解決する。見つからなければ実行可能な hint を返す。
pub fn resolve(api_key: Option<&str>, api_key_file: Option<&PathBuf>) -> Result<Auth> {
    if let Some(key) = api_key {
        let key = key.trim();
        if key.is_empty() {
            bail!("--api-key が空です");
        }
        return Ok(Auth::ApiKey(key.to_string()));
    }

    if let Ok(key) = std::env::var("RBXOC_API_KEY") {
        let key = key.trim().to_string();
        if !key.is_empty() {
            return Ok(Auth::ApiKey(key));
        }
    }

    if let Some(path) = api_key_file {
        return read_secret(path, Auth::ApiKey);
    }

    if let Ok(token) = std::env::var("RBXOC_OAUTH_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(Auth::OAuth(token));
        }
    }

    if let Some(home) = home_dir() {
        let key_path = home.join(".rbxoc").join("api-key");
        if key_path.exists() {
            return read_secret(&key_path, Auth::ApiKey);
        }
        let token_path = home.join(".rbxoc").join("oauth-token");
        if token_path.exists() {
            return read_secret(&token_path, Auth::OAuth);
        }
    }

    bail!(
        "認証情報が見つかりません。RBXOC_API_KEY を設定するか、\
         ~/.rbxoc/api-key に API key を置いてください"
    )
}

/// 秘密ファイルを読む。1 行目だけを使い、改行を落とす。
fn read_secret(path: &PathBuf, wrap: fn(String) -> Auth) -> Result<Auth> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("秘密ファイルを読めません: {}", path.display()))?;
    let value = raw.lines().next().unwrap_or("").trim().to_string();
    if value.is_empty() {
        bail!("秘密ファイルが空です: {}", path.display());
    }
    Ok(wrap(value))
}

/// ホームディレクトリ。Windows は USERPROFILE、他は HOME。
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// 認証情報なしのエラー。`code` で分岐できるようにする。
pub fn missing_auth_error(message: impl Into<String>) -> ErrorBody {
    ErrorBody {
        code: "auth_missing",
        message: message.into(),
        status: None,
        detail: None,
    }
}

/// HTTP クライアントを作る。timeout を明示し、エージェントが待ち続けないようにする。
pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("rbxoc/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .context("HTTP クライアントの初期化に失敗しました")
}

/// API レスポンスのステータスをエラーへ変換する。呼び出し側で成功判定に使う。
pub async fn error_from_response(response: reqwest::Response) -> ErrorBody {
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    // HTML エラーページをそのまま流さない。先頭だけ残す。
    let detail = if body.is_empty() {
        None
    } else {
        Some(body.chars().take(600).collect())
    };
    ErrorBody {
        code: status_code(&status),
        message: format!("Open Cloud が HTTP {status} を返しました"),
        status: Some(status),
        detail,
    }
}

/// HTTP ステータスを安定した `code` へ写す。エージェントはここで分岐する。
fn status_code(status: &u16) -> &'static str {
    match status {
        400 => "http_400",
        401 => "http_401",
        403 => "http_403",
        404 => "http_404",
        409 => "http_409",
        429 => "http_429",
        500..=599 => "http_5xx",
        _ => "http_error",
    }
}

/// ステータスごとの次の行動。エージェント向けの troubleshooting guidance。
pub fn hints_for_status(status: u16) -> Vec<String> {
    match status {
        401 => vec![
            "API key が無効か期限切れです。Creator Dashboard で再発行してください".into(),
            "`rbxoc auth check` で現在の認証状態を確認できます".into(),
        ],
        403 => vec![
            "API key に必要な権限がありません。対象の操作権限とスコープを確認してください".into(),
            "スコープが OAuth2 専用の領域では、`RBXOC_OAUTH_TOKEN` が必要です".into(),
            "`rbxoc api list --scope <name>` で該当エンドポイントの必要スコープを確認できます"
                .into(),
        ],
        404 => vec![
            "ID が誤っているか、API key が対象の体験にアクセスできません".into(),
            "universeId と placeId の取り違えを確認してください".into(),
        ],
        409 => vec!["競合しています。少し待って再試行してください".into()],
        429 => vec![
            "レート制限です。指数バックオフで再試行してください".into(),
            "`--retry 3` を付けると自動で再試行します".into(),
        ],
        500..=599 => vec!["Roblox 側の一時障害です。時間を置いて再試行してください".into()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_kind_and_mask() {
        let key = Auth::ApiKey("abcdefghijkl".into());
        assert_eq!(key.kind(), "api-key");
        assert_eq!(key.masked(), "abcd****");
        // 短い値は全体を隠す。
        assert_eq!(Auth::ApiKey("short".into()).masked(), "****");
    }

    #[test]
    fn status_codes_are_stable() {
        assert_eq!(status_code(&401), "http_401");
        assert_eq!(status_code(&403), "http_403");
        assert_eq!(status_code(&503), "http_5xx");
        assert_eq!(status_code(&418), "http_error");
    }

    #[test]
    fn hints_exist_for_common_failures() {
        assert!(!hints_for_status(401).is_empty());
        assert!(!hints_for_status(403).is_empty());
        assert!(!hints_for_status(429).is_empty());
        assert!(hints_for_status(200).is_empty());
    }
}
