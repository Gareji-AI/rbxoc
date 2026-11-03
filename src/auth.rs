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
//! 4. non-empty `RBXOC_OAUTH_TOKEN`
//!
//! The API key is loaded the same way Asphalt loads `ASPHALT_API_KEY`, but the
//! search is bounded by the active project. `load_dotenv` runs before argument
//! parsing and applies the nearest `.env` found between the working directory
//! and the project root (the nearest ancestor holding `.git`, `.rbxoc`,
//! `rokit.toml`, or `rbxoc.assets.toml`). Variables the process already has are
//! never overwritten. A `.env` above that boundary, or one in the user's home
//! directory, is never selected: Open Cloud API keys are universe-scoped, so
//! inheriting one from a parent workspace would aim a mutation at the wrong
//! universe.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
        if raw.chars().count() <= 8 {
            return "****".to_string();
        }
        format!("{}****", raw.chars().take(4).collect::<String>())
    }
}

/// Path of the `.env` that supplied `RBXOC_API_KEY`, when the process did not
/// already have that variable.
static LOADED_DOTENV_KEY: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Files that mark the root of the project a command belongs to. A `.env`
/// above the nearest one of these is outside the project's trust boundary.
const PROJECT_MARKERS: &[&str] = &[".git", ".rbxoc", "rokit.toml", "rbxoc.assets.toml"];

/// The nearest ancestor of `start` (inclusive) that holds a project marker.
fn project_root(start: &Path) -> Option<&Path> {
    start.ancestors().find(|directory| {
        PROJECT_MARKERS
            .iter()
            .any(|marker| directory.join(marker).exists())
    })
}

/// The user's home directory, when the platform reports one.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// The nearest `.env` between `start` and the active project root.
///
/// Returns `None` when the only candidates sit above the project boundary or
/// inside the home directory. Both are outside the credential contract: an API
/// key belongs to the project that owns the universe.
pub fn find_dotenv_from(start: &Path) -> Option<PathBuf> {
    let boundary = project_root(start);
    let home = home_dir();
    // Without a project marker there is no boundary to trust, so only the
    // working directory itself is considered. Walking up would let an
    // unrelated parent workspace supply a universe-scoped key.
    if boundary.is_none() {
        let candidate = start.join(".env");
        return candidate.is_file().then_some(candidate);
    }
    for directory in start.ancestors() {
        // Home and everything above it is out of scope, even when a project
        // marker happens to sit there.
        if let Some(home) = home.as_deref()
            && home.starts_with(directory)
        {
            break;
        }
        // Never walk past the project root.
        if let Some(boundary) = boundary
            && !directory.starts_with(boundary)
        {
            break;
        }
        let candidate = directory.join(".env");
        if candidate.is_file() {
            return Some(candidate);
        }
        if Some(directory) == boundary {
            break;
        }
    }
    None
}

/// Load the nearest `.env` inside the active project.
///
/// Parsing uses dotenvy's own rules. A variable the process already has from a
/// non-empty value wins; a variable that is present but blank does not, because
/// an exported empty `RBXOC_API_KEY=` would otherwise block the project file
/// forever.
pub fn load_dotenv() {
    let already_set = std::env::var("RBXOC_API_KEY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let path = std::env::current_dir()
        .ok()
        .and_then(|cwd| find_dotenv_from(&cwd));
    if let Some(file) = path.as_deref()
        && let Ok(pairs) = dotenvy::from_path_iter(file)
    {
        for (key, value) in pairs.flatten() {
            let occupied = std::env::var(&key)
                .ok()
                .is_some_and(|current| !current.trim().is_empty());
            if !occupied {
                // Safety: `load_dotenv` is the first statement of `main`, before
                // any other thread can read the environment.
                unsafe { std::env::set_var(key, value) };
            }
        }
    }
    let from_file = !already_set
        && std::env::var("RBXOC_API_KEY")
            .ok()
            .is_some_and(|value| !value.trim().is_empty());
    let _ = LOADED_DOTENV_KEY.set(path.filter(|_| from_file));
}

/// The credential and the exact artifact selected by the same resolution pass.
#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedAuth {
    pub auth: Auth,
    pub source: String,
}

impl ResolvedAuth {
    fn new(auth: Auth, source: impl Into<String>) -> Self {
        Self {
            auth,
            source: source.into(),
        }
    }
}

/// Resolve credentials. Returns an actionable hint when nothing is found.
pub fn resolve(api_key: Option<&str>, api_key_file: Option<&PathBuf>) -> Result<Auth> {
    resolve_with_source(api_key, api_key_file).map(|resolved| resolved.auth)
}

/// Resolve credentials and their source without recomputing precedence.
pub fn resolve_with_source(
    api_key: Option<&str>,
    api_key_file: Option<&PathBuf>,
) -> Result<ResolvedAuth> {
    resolve_with_sources(
        api_key,
        api_key_file,
        std::env::var("RBXOC_API_KEY").ok().as_deref(),
        std::env::var("RBXOC_OAUTH_TOKEN").ok().as_deref(),
        LOADED_DOTENV_KEY.get().and_then(|path| path.as_deref()),
    )
}

fn resolve_with_sources(
    api_key: Option<&str>,
    api_key_file: Option<&PathBuf>,
    env_key: Option<&str>,
    env_token: Option<&str>,
    dotenv_key_path: Option<&Path>,
) -> Result<ResolvedAuth> {
    if let Some(key) = api_key {
        let key = key.trim();
        if key.is_empty() {
            bail!("--api-key is empty");
        }
        return Ok(ResolvedAuth::new(
            Auth::ApiKey(key.to_string()),
            "--api-key",
        ));
    }

    if let Some(key) = env_key {
        let key = key.trim().to_string();
        if !key.is_empty() {
            let source = match dotenv_key_path {
                Some(path) => format!("{} (RBXOC_API_KEY)", path.display()),
                None => "RBXOC_API_KEY".to_string(),
            };
            return Ok(ResolvedAuth::new(Auth::ApiKey(key), source));
        }
    }

    if let Some(path) = api_key_file {
        return Ok(ResolvedAuth::new(
            read_secret(path, Auth::ApiKey)?,
            path.display().to_string(),
        ));
    }

    if let Some(token) = env_token {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(ResolvedAuth::new(Auth::OAuth(token), "RBXOC_OAUTH_TOKEN"));
        }
    }

    bail!(
        "no credentials found. Set RBXOC_API_KEY, or add RBXOC_API_KEY to the \
         project's .env (see `rbxoc auth check`)"
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

/// Shared client settings. Redirects are never followed: a 307/308 after a
/// mutation would replay the request, and reqwest cannot treat `x-api-key` as
/// a sensitive header across origins, so it would forward the credential.
/// Callers see the 3xx response instead and decide explicitly.
pub fn http_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(concat!("rbxoc/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
}

/// Build the HTTP client. Timeouts are explicit so an agent never waits forever.
pub fn http_client() -> Result<reqwest::Client> {
    http_client_builder()
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
        408 => "http_408",
        403 => "http_403",
        404 => "http_404",
        409 => "http_409",
        429 => "http_429",
        500..=599 => "http_5xx",
        _ => "http_error",
    }
}

/// Next actions per status. Troubleshooting guidance for agents.
///
/// `auth_kind` names the credential that was actually sent, so a 401 never calls
/// an OAuth token an API key and a 403 never suggests switching to the family
/// already in use. An empty kind means the caller did not know, and the wording
/// stays neutral rather than guessing.
pub fn hints_for_status(status: u16, auth_kind: &str) -> Vec<String> {
    let credential = match auth_kind {
        "oauth" => "OAuth token",
        "api-key" => "API key",
        _ => "credential",
    };
    match status {
        401 => vec![
            format!("The {credential} is invalid or expired. Reissue it in the Creator Dashboard"),
            "Run `rbxoc auth check` to inspect the current credential state".into(),
        ],
        403 => {
            let mut hints = vec![format!(
                "The {credential} lacks the required permission. Check the operation and scopes"
            )];
            // Only suggest the other family when the other family is not the
            // one already in use.
            match auth_kind {
                "oauth" => hints.push(
                    "OAuth2 is already in use; add the documented scopes to the token's app".into(),
                ),
                "api-key" => {
                    hints.push("Areas that require OAuth2 need `RBXOC_OAUTH_TOKEN` instead".into())
                }
                _ => {}
            }
            hints.push("Run `rbxoc api show <id>` to see the scopes an endpoint needs".into());
            hints
        }
        404 => vec![
            "Either the ID is wrong, or the credential cannot access that experience".into(),
            "Check for a swap between universeId and placeId".into(),
        ],
        409 => vec!["Conflicting state. Wait a moment and retry".into()],
        408 => vec![
            // Applies to reads and mutations alike: a timeout response proves
            // neither that a write landed nor that it did not.
            "The provider reported a request timeout; the request may still have been processed"
                .into(),
            "Inspect the target state before repeating a mutation; repeating a read is safe".into(),
        ],
        429 => vec![
            "Rate limited. Retry with exponential backoff".into(),
            "`--retry 3` adds automatic retries for replay-safe reads only".into(),
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
        assert_eq!(status_code(&408), "http_408");
        assert_eq!(status_code(&503), "http_5xx");
        assert_eq!(status_code(&418), "http_error");
    }

    #[test]
    fn the_production_client_is_built_without_redirects() {
        // The client must build; redirect refusal is covered end-to-end by
        // `http_contract_tests::redirects_never_replay_mutations_or_forward_the_api_key`.
        assert!(http_client().is_ok());
    }

    #[test]
    fn hints_exist_for_common_failures() {
        for kind in ["api-key", "oauth", ""] {
            assert!(!hints_for_status(401, kind).is_empty());
            assert!(!hints_for_status(403, kind).is_empty());
            assert!(!hints_for_status(429, kind).is_empty());
            assert!(hints_for_status(200, kind).is_empty());
        }
    }

    #[test]
    fn status_hints_describe_the_credential_that_was_sent() {
        // A 401 must never call an OAuth token an API key.
        let oauth = hints_for_status(401, "oauth").join(" ");
        assert!(oauth.contains("OAuth token"), "{oauth}");
        assert!(!oauth.contains("API key"), "{oauth}");
        let key = hints_for_status(401, "api-key").join(" ");
        assert!(key.contains("API key"), "{key}");
        assert!(!key.contains("OAuth token"), "{key}");

        // A 403 must not suggest switching to the family already in use.
        let oauth403 = hints_for_status(403, "oauth").join(" ");
        assert!(
            !oauth403.contains("need `RBXOC_OAUTH_TOKEN` instead"),
            "{oauth403}"
        );
        assert!(oauth403.contains("OAuth2 is already in use"), "{oauth403}");
        let key403 = hints_for_status(403, "api-key").join(" ");
        assert!(key403.contains("RBXOC_OAUTH_TOKEN"), "{key403}");

        // An unknown family stays neutral instead of claiming a method.
        let unknown = hints_for_status(401, "").join(" ");
        assert!(unknown.contains("credential"), "{unknown}");
        assert!(!unknown.contains("API key"), "{unknown}");
        assert!(!unknown.contains("OAuth"), "{unknown}");
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
        // A variable already in the process wins. dotenvy does not override it,
        // so the source names the variable rather than the file.
        let resolved = resolve_with_sources(None, None, Some("from-env"), None, None);

        assert_eq!(
            resolved.unwrap(),
            ResolvedAuth::new(Auth::ApiKey("from-env".into()), "RBXOC_API_KEY")
        );
    }

    #[test]
    fn mask_counts_characters_not_bytes_for_both_auth_families() {
        for (secret, expected) in [
            ("12345678", "****"),
            ("123456789", "1234****"),
            ("日本語abcde", "****"),
            ("日本語abcdef", "日本語a****"),
            ("😀😃😄😁abcdefgh", "😀😃😄😁****"),
            ("", "****"),
        ] {
            assert_eq!(Auth::ApiKey(secret.into()).masked(), expected);
            assert_eq!(Auth::OAuth(secret.into()).masked(), expected);
        }
    }

    #[test]
    fn every_resolution_branch_reports_the_selected_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("explicit-key");
        std::fs::write(&file, "explicit-file").unwrap();
        let dotenv = dir.path().join(".env");
        let source = format!("{} (RBXOC_API_KEY)", dotenv.display());
        let resolve = |arg, file, env, oauth| {
            resolve_with_sources(arg, file, env, oauth, Some(&dotenv)).unwrap()
        };
        assert_eq!(
            resolve(Some("arg"), Some(&file), Some("env"), Some("oauth")),
            ResolvedAuth::new(Auth::ApiKey("arg".into()), "--api-key")
        );
        assert_eq!(
            resolve(None, Some(&file), Some("env"), Some("oauth")),
            ResolvedAuth::new(Auth::ApiKey("env".into()), source.clone())
        );
        for env in [None, Some(""), Some("  ")] {
            assert_eq!(
                resolve(None, Some(&file), env, Some("oauth")),
                ResolvedAuth::new(
                    Auth::ApiKey("explicit-file".into()),
                    file.display().to_string()
                )
            );
            assert_eq!(
                resolve(None, None, env, Some("oauth")),
                ResolvedAuth::new(Auth::OAuth("oauth".into()), "RBXOC_OAUTH_TOKEN")
            );
            for oauth in [None, Some(""), Some("  ")] {
                let missing = resolve_with_sources(None, None, env, oauth, Some(&dotenv))
                    .unwrap_err()
                    .to_string();
                assert!(missing.contains("RBXOC_API_KEY"));
                assert!(!missing.contains("explicit-file"));
            }
        }
        assert!(resolve_with_sources(None, None, Some(""), Some(""), None).is_err());
        assert!(resolve_with_sources(Some(" "), None, Some("env"), None, None).is_err());
        // An explicit file wins even without a project `.env`.
        assert_eq!(
            resolve_with_sources(None, Some(&file), None, None, None)
                .unwrap()
                .source,
            file.display().to_string()
        );
    }

    #[test]
    fn dotenvy_reads_the_quoted_api_key() {
        // Same parser Asphalt uses. A later empty assignment replaces the
        // earlier value; other keys stay in the file and are not the API key.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(
            &path,
            "\
# comment
OTHER=secret
export RBXOC_API_KEY=\"quoted key\" # not part of the key
RBXOC_OAUTH_TOKEN='token value'
",
        )
        .unwrap();
        let pairs: Vec<(String, String)> = dotenvy::from_path_iter(&path)
            .unwrap()
            .map(|pair| pair.unwrap())
            .collect();
        assert_eq!(
            pairs
                .iter()
                .find(|(key, _)| key == "RBXOC_API_KEY")
                .map(|(_, value)| value.as_str()),
            Some("quoted key")
        );
        assert!(pairs.iter().any(|(key, _)| key == "OTHER"));
    }

    #[test]
    fn dotenv_path_is_reported_only_for_the_loaded_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        let resolved =
            resolve_with_sources(None, None, Some("from-dotenv"), None, Some(&path)).unwrap();
        assert_eq!(resolved.auth, Auth::ApiKey("from-dotenv".into()));
        assert!(resolved.source.ends_with("(RBXOC_API_KEY)"));
        assert!(
            resolved
                .source
                .contains(path.display().to_string().as_str())
        );
        assert!(!resolved.source.contains("from-dotenv"));
    }

    #[test]
    fn dotenv_lookup_stops_at_the_project_boundary() {
        let root = tempfile::tempdir().unwrap();
        let ancestor = root.path().join("ancestor");
        let project = ancestor.join("project");
        let nested = project.join("subdir");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::write(ancestor.join(".env"), "RBXOC_API_KEY=ancestor\n").unwrap();

        // No `.env` inside the project: the ancestor file is out of reach.
        assert_eq!(find_dotenv_from(&nested), None);

        // The project-local file is found from a nested directory.
        let local = project.join(".env");
        std::fs::write(&local, "RBXOC_API_KEY=local\n").unwrap();
        assert_eq!(find_dotenv_from(&nested), Some(local.clone()));

        // A closer `.env` still wins over the project-root one.
        let closer = nested.join(".env");
        std::fs::write(&closer, "OTHER=1\n").unwrap();
        assert_eq!(find_dotenv_from(&nested), Some(closer));
    }

    #[test]
    fn dotenv_lookup_without_a_project_marker_stays_local() {
        // Without `.git` or another marker the search must not escape into an
        // unrelated parent tree.
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(root.path().join(".env"), "RBXOC_API_KEY=ancestor\n").unwrap();

        let found = find_dotenv_from(&project);
        assert!(
            found.as_deref() != Some(root.path().join(".env").as_path()),
            "an ancestor .env outside the project boundary was selected: {found:?}"
        );
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
