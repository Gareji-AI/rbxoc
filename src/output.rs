//! Output contract.
//!
//! The core of an agent-facing CLI. Every command returns the same envelope.
//!
//! - `ok`: whether it succeeded. Matches the process exit code.
//! - `command`: the logical command name. For log correlation.
//! - `summary`: a one-line summary. The first thing an agent reads.
//! - `data`: structured result. The schema is fixed per command.
//! - `hints`: available next actions. Always present on failure.
//! - `error`: failure detail. Branch on `code`.
//!
//! Text output is rendered from the same source. `Output` is always the path,
//! so the JSON and text views never diverge.

use serde::Serialize;
use serde_json::Value;

/// Machine-readable failure representation.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    /// Stable identifier for branching. Example: `auth_missing`, `http_401`.
    pub code: &'static str,
    /// What went wrong. One line.
    pub message: String,
    /// HTTP status. `None` when no API call was made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// Body returned by the API. Truncated raw text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The envelope every command returns.
#[derive(Debug, Clone, Serialize)]
pub struct Output {
    pub ok: bool,
    pub command: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Output {
    /// Success. Attach `data` when there is any.
    pub fn success(
        command: impl Into<String>,
        summary: impl Into<String>,
        data: Option<Value>,
    ) -> Self {
        Self {
            ok: true,
            command: command.into(),
            summary: summary.into(),
            data,
            hints: Vec::new(),
            error: None,
        }
    }

    /// Append available next actions. Agents read these to decide what to do.
    pub fn with_hints(mut self, hints: impl IntoIterator<Item = String>) -> Self {
        self.hints.extend(hints);
        self
    }

    /// Human-readable text rendering. Carries the same information as JSON.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.summary);
        if let Some(data) = &self.data {
            let pretty = serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string());
            out.push('\n');
            out.push_str(&pretty);
        }
        if !self.hints.is_empty() {
            out.push_str("\n\nhints:");
            for hint in &self.hints {
                out.push_str("\n  - ");
                out.push_str(hint);
            }
        }
        if let Some(error) = &self.error {
            out.push_str(&format!("\n\nerror[{}]: {}", error.code, error.message));
            if let Some(status) = error.status {
                out.push_str(&format!("\n  status: {status}"));
            }
            if let Some(detail) = &error.detail {
                out.push_str(&format!("\n  detail: {detail}"));
            }
        }
        out
    }
}

/// Output format for the whole CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// Agent-facing. Stable JSON.
    Json,
    /// Human-facing. Summary plus pretty JSON.
    Text,
}

impl Format {
    /// Default from the `RBXOC_FORMAT` environment variable. Pin it in CI and
    /// agent runs.
    pub fn from_env_or_json() -> Self {
        match std::env::var("RBXOC_FORMAT").as_deref() {
            Ok("text") => Format::Text,
            _ => Format::Json,
        }
    }
}
