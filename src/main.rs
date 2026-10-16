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
mod open_cloud_errors;
mod output;

use clap::Parser;
use std::process::ExitCode;

use cli::Cli;
use output::Format;

#[tokio::main]
async fn main() -> ExitCode {
    let (format, out) = match Cli::try_parse() {
        Ok(cli) => (cli.resolve_format(), cli.run().await),
        Err(err)
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            return if err.print().is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(70)
            };
        }
        Err(err) => (
            // Parsing has not produced a trustworthy format option yet. Usage
            // failures always use the stable JSON envelope on stdout.
            Format::Json,
            ctx::failure(
                "usage",
                output::ErrorBody {
                    code: "usage_error",
                    message: "Invalid command-line arguments".into(),
                    status: None,
                    detail: Some(err.to_string()),
                    refs: None,
                },
            )
            .with_hints(vec![
                "Run `rbxoc --help` or `rbxoc <command> --help`".into(),
            ]),
        ),
    };

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
            Some("usage_error" | "unknown_endpoint" | "unknown_auth" | "unknown_kind") => 64,
            _ => 1,
        };
        ExitCode::from(code)
    }
}
