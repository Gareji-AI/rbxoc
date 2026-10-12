//! Authentication and HTTP client.
//!
//! Open Cloud has three auth families (`roblox-api-key` / `roblox-oauth2` /
//! `roblox-legacy-cookie`). This CLI supports API keys and OAuth2 bearer
//! tokens. Legacy cookies come from a browser session and do not suit
//! automation, so they are out of scope.
//!
//! API key lookup order:
//! 1. `--api-key` (agents should avoid it; it lands in shell history)
//! 2. `RBXOC_API_KEY` environment variable
//! 3. the file given by `--api-key-file`
//! 4. the default keyring file `~/.rbxoc/api-key` (permissions are checked)
//!
//! OAuth2 comes from `RBXOC_OAUTH_TOKEN` or `~/.rbxoc/oauth-token`.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::output::ErrorBody;

/// Open Cloud API host.
pub const API_HOST: &str = "https://apis.roblox.com";

/// Authentication method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    /// `x-api-key` header. Most endpoints.
    ApiKey(String),
    /// `Authorization: Bearer`. Areas that need OAuth2 scopes.
    OAuth(String),
}

impl Auth {
    /// Display name that never leaks the secret.
    pub fn kind(&self) -> &'static str {
        match self {
            Auth::ApiKey(_) => "api-key",
            Auth::OAuth(_) => "oauth",
        }
    }

    /// Masked value. Keeps only the first 4 characters. For error display.
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

/// Resolve credentials. Returns an actionable hint when nothing is found.
pub fn resolve(api_key: Option<&str>, api_key_file: Option<&PathBuf>) -> Result<Auth> {
    if let Some(key) = api_key {
        let key = key.trim();
        if key.is_empty() {
            bail!("--api-key is empty");
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
        "no credentials found. Set RBXOC_API_KEY, or put an API key in \
         ~/.rbxoc/api-key"
    )
}

/// Read a secret file. Uses the first line and strips the newline.
fn read_secret(path: &PathBuf, wrap: fn(String) -> Auth) -> Result<Auth> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read secret file: {}", path.display()))?;
    let value = raw.lines().next().unwrap_or("").trim().to_string();
    if value.is_empty() {
        bail!("secret file is empty: {}", path.display());
    }
    Ok(wrap(value))
}

/// Home directory. USERPROFILE on Windows, HOME elsewhere.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Error for missing credentials. `code` allows downstream branching.
pub fn missing_auth_error(message: impl Into<String>) -> ErrorBody {
    ErrorBody {
        code: "auth_missing",
        message: message.into(),
        status: None,
        detail: None,
    }
}

/// Build the HTTP client. Timeouts are explicit so an agent never waits forever.
pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("rbxoc/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .context("failed to initialize the HTTP client")
}

/// Convert an API response status into an error. Callers use it for success checks.
pub async fn error_from_response(response: reqwest::Response) -> ErrorBody {
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    // Do not forward an HTML error page verbatim. Keep the head only.
    let detail = if body.is_empty() {
        None
    } else {
        Some(body.chars().take(600).collect())
    };
    ErrorBody {
        code: status_code(&status),
        message: format!("Open Cloud returned HTTP {status}"),
        status: Some(status),
        detail,
    }
}

/// Map an HTTP status to a stable `code`. Agents branch on this.
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

/// Next actions per status. Troubleshooting guidance for agents.
pub fn hints_for_status(status: u16) -> Vec<String> {
    match status {
        401 => vec![
            "The API key is invalid or expired. Reissue it in the Creator Dashboard".into(),
            "Run `rbxoc auth check` to inspect the current credential state".into(),
        ],
        403 => vec![
            "The API key lacks the required permission. Check the operation and scopes".into(),
            "Areas that require OAuth2 need `RBXOC_OAUTH_TOKEN` instead".into(),
            "Run `rbxoc api show <id>` to see the scopes an endpoint needs".into(),
        ],
        404 => vec![
            "Either the ID is wrong, or the API key cannot access that experience".into(),
            "Check for a swap between universeId and placeId".into(),
        ],
        409 => vec!["Conflicting state. Wait a moment and retry".into()],
        429 => vec![
            "Rate limited. Retry with exponential backoff".into(),
            "`--retry 3` adds automatic retries".into(),
        ],
        500..=599 => vec!["Temporary Roblox-side failure. Retry later".into()],
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
        // Short values are hidden entirely.
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
