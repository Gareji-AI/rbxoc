//! rbxoc - Roblox Open Cloud CLI for coding agents.
//!
//! Design pillars:
//! 1. Deterministic discovery - `rbxoc api list` / `api show <id>` return a
//!    self-describing catalog
//! 2. Machine-readable output - every command returns the same JSON envelope
//! 3. Troubleshooting guidance - failures always carry `hints`
//! 4. Secret handling - environment variables and files take priority over args

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

    // JSON is the stable machine-readable surface. text is for humans.
    match format {
        Format::Json => match serde_json::to_string_pretty(&out) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("failed to encode output: {e}");
                return ExitCode::from(70);
            }
        },
        Format::Text => println!("{}", out.render_text()),
    }

    if out.ok {
        ExitCode::SUCCESS
    } else {
        // Let callers distinguish missing credentials from other failures.
        let code = match out.error.as_ref().map(|e| e.code) {
            Some("auth_missing") => 2,
            Some("unknown_endpoint") | Some("unknown_auth") | Some("unknown_kind") => 64,
            _ => 1,
        };
        ExitCode::from(code)
    }
}
