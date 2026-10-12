//! 出力契約。
//!
//! エージェント向け CLI の中核。すべてのコマンドが同じ封筒（envelope）を返す。
//!
//! - `ok`: 成功したか。終了コードと一致する。
//! - `command`: 実行した論理コマンド名。ログの突き合わせ用。
//! - `summary`: 一行の要約。エージェントが最初に読む。
//! - `data`: 構造化された結果。スキーマはコマンドごとに固定。
//! - `hints`: 次に取れる行動。失敗時は必ず入れる。
//! - `error`: 失敗の詳細。`code` で分岐できる。
//!
//! 人間向けの text 出力も同じ情報源から描画する。JSON と text で内容が
//! ずれないよう、必ず `Output` を経由する。

use serde::Serialize;
use serde_json::Value;

/// 失敗の機械可読な表現。
#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    /// 分岐に使う安定した識別子。例: `auth_missing`, `http_401`。
    pub code: &'static str,
    /// 何が起きたか。一行。
    pub message: String,
    /// HTTP ステータス。API 呼び出しでなければ `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// API が返した本文。切り詰めた生テキスト。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// すべてのコマンドが返す封筒。
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
    /// 成功。`data` があれば載せる。
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

    /// 次に取れる行動を足す。エージェントはここを読んで進む。
    pub fn with_hints(mut self, hints: impl IntoIterator<Item = String>) -> Self {
        self.hints.extend(hints);
        self
    }

    /// 人間向けの text 描画。JSON と同じ情報を出す。
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

/// CLI 全体の出力形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// エージェント向け。安定した JSON。
    Json,
    /// 人間向け。要約と整形 JSON。
    Text,
}

impl Format {
    /// 環境変数 `RBXOC_FORMAT` の既定値。CI やエージェント実行で固定する。
    pub fn from_env_or_json() -> Self {
        match std::env::var("RBXOC_FORMAT").as_deref() {
            Ok("text") => Format::Text,
            _ => Format::Json,
        }
    }
}
