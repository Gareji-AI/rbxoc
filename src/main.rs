//! rbxoc — Roblox Open Cloud CLI for coding agents.
//!
//! 設計の柱:
//! 1. 決定論的発見 — `rbxoc api list` / `api show <id>` が自己記述カタログを返す
//! 2. 機械可読出力 — すべてのコマンドが同じ JSON 封筒を返す
//! 3. troubleshooting guidance — 失敗には必ず `hints` が付く
//! 4. 秘密値の扱い — 引数より環境変数とファイルを優先する

mod auth;
mod catalog;
mod cli;
mod cmd;
mod ctx;
mod output;

use clap::Parser;
use std::process::ExitCode;

use cli::Cli;
use output::Format;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = cli.resolve_format();
    let out = cli.run().await;

    // JSON は安定した機械可読出力。text は人間向け。
    match format {
        Format::Json => match serde_json::to_string_pretty(&out) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("出力のエンコードに失敗しました: {e}");
                return ExitCode::from(70);
            }
        },
        Format::Text => println!("{}", out.render_text()),
    }

    if out.ok {
        ExitCode::SUCCESS
    } else {
        // 認証欠落を呼び出し側が区別できるようにする。
        let code = match out.error.as_ref().map(|e| e.code) {
            Some("auth_missing") => 2,
            Some("unknown_endpoint") | Some("unknown_auth") | Some("unknown_kind") => 64,
            _ => 1,
        };
        ExitCode::from(code)
    }
}
