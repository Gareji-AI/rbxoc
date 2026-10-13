//! Authentication and HTTP client.
//!
//! Open Cloud has three auth families (`roblox-api-key` / `roblox-oauth2` /
//! `roblox-legacy-cookie`). This CLI supports API keys and OAuth2 bearer
//! tokens. Legacy cookies come from a browser session and do not suit
//! automation, so they are out of scope.
//!
//! An Open Cloud API key is scoped to one universe (or a group of them), so a
//! single global key is the wrong model: it makes it easy to run one game's
//! commands against another game's key. Credentials are therefore resolved
//! from the current project first, then from the environment.
//!
//! Lookup order:
//! 1. `--api-key` (agents should avoid it; it lands in shell history)
//! 2. `RBXOC_API_KEY` environment variable
//! 3. the file given by `--api-key-file`
//! 4. `.rbxoc/api-key` walking up from the working directory, stopping at the
//!    first `.rbxoc` found. This keeps one key per project.
//!
//! OAuth2 comes from `RBXOC_OAUTH_TOKEN` or `.rbxoc/oauth-token` in the same
//! project-local directory. There is deliberately no home-directory fallback:
//! a key that applies to every repository invites cross-game mistakes.

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

/// Directory name that marks a project's credentials.
pub const CREDENTIAL_DIR: &str = ".rbxoc";

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

    // Project-local lookup. An API key is scoped to one universe, so it belongs
    // beside the project it targets, not in the home directory.
    if let Some(dir) = find_credential_dir() {
        let key_path = dir.join("api-key");
        if key_path.exists() {
            return read_secret(&key_path, Auth::ApiKey);
        }
        let token_path = dir.join("oauth-token");
        if token_path.exists() {
            return read_secret(&token_path, Auth::OAuth);
        }
    }

    bail!(
        "no credentials found. Set RBXOC_API_KEY, or create {CREDENTIAL_DIR}/api-key \
         in this project (see `rbxoc auth check`)"
    )
}

/// Walk up from the working directory to the first `.rbxoc` directory.
///
/// Stopping at the first match is what keeps keys per-project: a nested
/// repository with its own `.rbxoc` never inherits the parent's key.
pub fn find_credential_dir() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(CREDENTIAL_DIR);
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
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

/// Error for missing credentials. `code` allows downstream branching.
pub fn missing_auth_error(message: impl Into<String>) -> ErrorBody {
    ErrorBody {
        code: "auth_missing",
        message: message.into(),
        status: None,
        detail: None,
        refs: None,
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
    crate::open_cloud_errors::error_from_http(response).await
}

/// Map an HTTP status to a stable `code`. Agents branch on this.
pub fn status_code(status: &u16) -> &'static str {
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

    #[test]
    fn credential_dir_is_found_beside_the_project() {
        // A `.rbxoc` directory in the working tree is picked up.
        let dir = tempfile::tempdir().unwrap();
        let creds = dir.path().join(CREDENTIAL_DIR);
        std::fs::create_dir(&creds).unwrap();
        std::fs::write(
            creds.join("api-key"),
            "test-key-value
",
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        // Serialize the cwd change; other tests read the working directory too.
        std::env::set_current_dir(dir.path()).unwrap();
        let found = find_credential_dir();
        std::env::set_current_dir(previous).unwrap();

        assert_eq!(found.as_deref(), Some(creds.as_path()));
    }

    #[test]
    fn nested_project_does_not_inherit_a_parent_key() {
        // A child directory with its own `.rbxoc` must win over the parent's.
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(CREDENTIAL_DIR)).unwrap();
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let child_creds = child.join(CREDENTIAL_DIR);
        std::fs::create_dir(&child_creds).unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&child).unwrap();
        let found = find_credential_dir();
        std::env::set_current_dir(previous).unwrap();

        assert_eq!(found.as_deref(), Some(child_creds.as_path()));
    }

    #[test]
    fn secret_file_uses_only_the_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api-key");
        std::fs::write(
            &path,
            "the-key
ignored-trailing-line
",
        )
        .unwrap();
        let auth = read_secret(&path, Auth::ApiKey).unwrap();
        assert_eq!(auth, Auth::ApiKey("the-key".into()));
    }

    #[test]
    fn env_var_takes_priority_over_project_file() {
        // The environment is the CI path, so it must win over a checked-in
        // project file. The reported source then names the variable.
        let dir = tempfile::tempdir().unwrap();
        let creds = dir.path().join(CREDENTIAL_DIR);
        std::fs::create_dir(&creds).unwrap();
        std::fs::write(
            creds.join("api-key"),
            "from-file
",
        )
        .unwrap();

        // SAFETY: single-threaded test setup, restored before the test ends.
        let previous = std::env::var("RBXOC_API_KEY").ok();
        unsafe { std::env::set_var("RBXOC_API_KEY", "from-env") };

        let previous_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let resolved = resolve(None, None);
        std::env::set_current_dir(previous_dir).unwrap();

        match previous {
            Some(v) => unsafe { std::env::set_var("RBXOC_API_KEY", v) },
            None => unsafe { std::env::remove_var("RBXOC_API_KEY") },
        }

        assert_eq!(resolved.unwrap(), Auth::ApiKey("from-env".into()));
    }

    #[test]
    fn empty_secret_file_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api-key");
        std::fs::write(
            &path, "   
",
        )
        .unwrap();
        assert!(read_secret(&path, Auth::ApiKey).is_err());
    }
}
