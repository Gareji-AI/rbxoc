//! Command definitions and dispatch.
//!
//! Agent-facing CLI principles:
//! - every command returns the same envelope (`Output`)
//! - discovery comes from the self-describing catalog via `rbxoc api list`
//! - failures always carry `hints`
//! - credentials prefer environment variables and files over arguments

use clap::{Args, Parser, Subcommand};

use crate::auth;
use crate::catalog;
use crate::cmd;
use crate::ctx::{Ctx, failure};
use crate::output::{Format, Output};

#[derive(Debug, Parser)]
#[command(
    name = "rbxoc",
    version,
    about = "CLI for the Roblox Open Cloud API, designed for coding agents",
    long_about = "Operate the Roblox Open Cloud API from the command line.\n\
                  Every command returns a JSON envelope, and failures carry\n\
                  next actions in `hints`. Discover what is possible with\n\
                  `rbxoc api list`."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Output format. Defaults to json. `RBXOC_FORMAT` also sets it.
    #[arg(long, value_enum, global = true)]
    pub format: Option<Format>,

    /// API key. Falls back to RBXOC_API_KEY or ~/.rbxoc/api-key.
    #[arg(long, global = true, env = "RBXOC_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,

    /// File containing the API key.
    #[arg(long, global = true, value_name = "PATH")]
    pub api_key_file: Option<std::path::PathBuf>,

    /// Automatic retry count for 429 / 5xx.
    #[arg(long, global = true, default_value_t = 2)]
    pub retry: u32,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect the credential state
    Auth(AuthArgs),

    /// Read the API catalog (discover what is possible)
    Api(ApiArgs),

    /// Publish and fetch places
    Place(PlaceArgs),

    /// Work with assets
    Asset(AssetArgs),

    /// Replace thumbnails
    Thumbnail(ThumbnailArgs),

    /// Game servers and logs
    Server(ServerArgs),

    /// Analytics queries
    Analytics(AnalyticsArgs),

    /// DataStores
    Datastore(DatastoreArgs),

    /// Game Passes
    Gamepass(GamepassArgs),

    /// Developer Products
    Devproduct(DevproductArgs),

    /// Universes
    Universe(UniverseArgs),
}

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Show how credentials resolved, and probe them if a universe is given
    Check {
        /// Universe ID to probe. Without it, only resolution is reported.
        #[arg(long)]
        universe_id: Option<u64>,
    },
}

#[derive(Debug, Args)]
pub struct ApiArgs {
    #[command(subcommand)]
    pub command: ApiCommand,
}

#[derive(Debug, Subcommand)]
pub enum ApiCommand {
    /// List every endpoint
    List {
        /// Only implemented endpoints
        #[arg(long)]
        implemented: bool,
        /// Substring filter
        #[arg(long)]
        query: Option<String>,
        /// Filter by auth method (api-key / oauth / legacy-cookie)
        #[arg(long)]
        auth: Option<String>,
    },
    /// Show one endpoint in detail
    Show {
        /// Endpoint ID (for example: place.publish)
        id: String,
    },
    /// List mutating operations. Review them before running.
    Mutating,
}

#[derive(Debug, Args)]
pub struct PlaceArgs {
    #[command(subcommand)]
    pub command: PlaceCommand,
}

#[derive(Debug, Subcommand)]
pub enum PlaceCommand {
    /// Save or publish a place from an RBXL / RBXLX file
    Publish {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        place_id: u64,
        /// File to send
        #[arg(long, value_name = "PATH")]
        file: std::path::PathBuf,
        /// Also go live. Without it, the place is only saved.
        #[arg(long)]
        published: bool,
    },
    /// Fetch a place's metadata
    Get {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        place_id: u64,
    },
    /// Fetch a place's version history
    History {
        #[arg(long)]
        place_id: u64,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
}

#[derive(Debug, Args)]
pub struct AssetArgs {
    #[command(subcommand)]
    pub command: AssetCommand,
}

#[derive(Debug, Subcommand)]
pub enum AssetCommand {
    /// Upload a new asset
    Create {
        #[arg(long, value_name = "PATH")]
        file: std::path::PathBuf,
        #[arg(long)]
        display_name: String,
        #[arg(long, default_value = "")]
        description: String,
        /// Create under your own account
        #[arg(long)]
        user_id: Option<u64>,
        /// Create as a group asset
        #[arg(long)]
        group_id: Option<u64>,
        /// Set assetType explicitly when the extension is not conclusive
        #[arg(long)]
        asset_type: Option<String>,
    },
    /// Update an asset's content and metadata
    Update {
        #[arg(long)]
        asset_id: u64,
        #[arg(long, value_name = "PATH")]
        file: std::path::PathBuf,
        #[arg(long)]
        asset_type: Option<String>,
        #[arg(long)]
        display_name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Square PNG / JPEG
        #[arg(long, value_name = "PATH")]
        icon: Option<std::path::PathBuf>,
        #[arg(long, value_name = "PATH")]
        preview: Option<std::path::PathBuf>,
    },
    /// Fetch asset metadata
    Get {
        #[arg(long)]
        asset_id: u64,
        #[arg(long)]
        read_mask: Option<String>,
    },
    /// List an asset's versions
    Versions {
        #[arg(long)]
        asset_id: u64,
        #[arg(long, default_value_t = 20)]
        max_page_size: u32,
    },
    /// Roll an asset back to a given version
    Rollback {
        #[arg(long)]
        asset_id: u64,
        #[arg(long)]
        version: u64,
    },
    /// Archive an asset
    Archive {
        #[arg(long)]
        asset_id: u64,
    },
    /// Check the status of an asynchronous operation
    Operation {
        #[arg(long)]
        operation_id: String,
    },
}

#[derive(Debug, Args)]
pub struct ThumbnailArgs {
    #[command(subcommand)]
    pub command: ThumbnailCommand,
}

#[derive(Debug, Subcommand)]
pub enum ThumbnailCommand {
    /// Replace homepage thumbnails
    Upload {
        #[arg(long)]
        universe_id: u64,
        #[arg(long, value_name = "PATH", required = true)]
        files: Vec<std::path::PathBuf>,
    },
    /// Check upload processing status
    Status {
        #[arg(long)]
        universe_id: u64,
        #[arg(long, value_delimiter = ',', required = true)]
        operation_ids: Vec<String>,
    },
    /// List homepage thumbnails
    List {
        #[arg(long)]
        universe_id: u64,
    },
    /// Delete thumbnails
    Delete {
        #[arg(long)]
        universe_id: u64,
        #[arg(long, value_delimiter = ',', required = true)]
        thumbnail_ids: Vec<String>,
    },
}

#[derive(Debug, Args)]
pub struct ServerArgs {
    #[command(subcommand)]
    pub command: ServerCommand,
}

#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    /// List game servers
    List {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        place_id: u64,
        #[arg(long)]
        version: u64,
    },
    /// Fetch game server logs
    Logs {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        place_id: u64,
        #[arg(long)]
        version: u64,
        #[arg(long)]
        job_id: String,
        /// Entries to show. 0 means all.
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Show everything (same as --limit 0)
        #[arg(long)]
        full: bool,
    },
    /// Restart servers
    Restart {
        #[arg(long)]
        universe_id: u64,
        /// Only forecast the impact
        #[arg(long)]
        dry_run: bool,
    },
    /// Forecast a restart's impact
    RestartForecast {
        #[arg(long)]
        universe_id: u64,
    },
}

#[derive(Debug, Args)]
pub struct AnalyticsArgs {
    #[command(subcommand)]
    pub command: AnalyticsCommand,
}

#[derive(Debug, Subcommand)]
pub enum AnalyticsCommand {
    /// Query time-series metrics
    Metrics {
        #[arg(long)]
        universe_id: u64,
        /// Query JSON
        #[arg(long)]
        query: String,
    },
    /// Fetch the result of an asynchronous query
    Operation {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        operation_id: String,
    },
    /// List common metrics
    Catalog,
}

#[derive(Debug, Args)]
pub struct DatastoreArgs {
    #[command(subcommand)]
    pub command: DatastoreCommand,
}

#[derive(Debug, Subcommand)]
pub enum DatastoreCommand {
    Get {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        store: String,
        #[arg(long)]
        entry: String,
        #[arg(long)]
        scope: Option<String>,
    },
    Set {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        store: String,
        #[arg(long)]
        entry: String,
        #[arg(long)]
        scope: Option<String>,
        /// JSON string
        #[arg(long)]
        value: String,
        /// Allow creating a new entry
        #[arg(long)]
        create: bool,
    },
    Delete {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        store: String,
        #[arg(long)]
        entry: String,
        #[arg(long)]
        scope: Option<String>,
    },
    List {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        store: String,
        #[arg(long, default_value_t = 50)]
        max_page_size: u32,
    },
}

#[derive(Debug, Args)]
pub struct GamepassArgs {
    #[command(subcommand)]
    pub command: GamepassCommand,
}

#[derive(Debug, Subcommand)]
pub enum GamepassCommand {
    List {
        #[arg(long)]
        universe_id: u64,
    },
    Create {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        /// Integer Robux
        #[arg(long)]
        price: Option<u64>,
        #[arg(long, value_name = "PATH")]
        image: Option<std::path::PathBuf>,
    },
    Update {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        game_pass_id: u64,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        price: Option<u64>,
        #[arg(long, value_name = "PATH")]
        image: Option<std::path::PathBuf>,
    },
}

#[derive(Debug, Args)]
pub struct DevproductArgs {
    #[command(subcommand)]
    pub command: DevproductCommand,
}

#[derive(Debug, Subcommand)]
pub enum DevproductCommand {
    List {
        #[arg(long)]
        universe_id: u64,
    },
    Create {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        price: Option<u64>,
        #[arg(long, value_name = "PATH")]
        image: Option<std::path::PathBuf>,
    },
}

#[derive(Debug, Args)]
pub struct UniverseArgs {
    #[command(subcommand)]
    pub command: UniverseCommand,
}

#[derive(Debug, Subcommand)]
pub enum UniverseCommand {
    Get {
        #[arg(long)]
        universe_id: u64,
    },
    /// Publish a cross-server message
    PublishMessage {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        topic: String,
        /// JSON string
        #[arg(long)]
        message: String,
    },
}

impl Cli {
    /// Resolve the output format. Argument > environment > default.
    pub fn resolve_format(&self) -> Format {
        self.format.unwrap_or_else(Format::from_env_or_json)
    }

    /// Run the command. Commands that need no credentials return early.
    pub async fn run(self) -> Output {
        // The catalog needs no credentials. It is the first thing an agent calls.
        if let Command::Api(args) = &self.command {
            return run_api(args).await;
        }

        if let Command::Auth(AuthArgs { command }) = &self.command {
            return run_auth(self.api_key.as_deref(), self.api_key_file.as_ref(), command).await;
        }

        let auth = match auth::resolve(self.api_key.as_deref(), self.api_key_file.as_ref()) {
            Ok(a) => a,
            Err(e) => {
                return failure("auth", auth::missing_auth_error(format!("{e:#}")));
            }
        };
        let ctx = match Ctx::new(auth, self.retry) {
            Ok(c) => c,
            Err(e) => {
                return failure(
                    "init",
                    crate::output::ErrorBody {
                        code: "client_init_failed",
                        message: format!("{e:#}"),
                        status: None,
                        detail: None,
                    },
                );
            }
        };

        match self.command {
            Command::Api(_) | Command::Auth(_) => unreachable!("handled above"),
            Command::Place(args) => match args.command {
                PlaceCommand::Publish {
                    universe_id,
                    place_id,
                    file,
                    published,
                } => cmd::place::publish(&ctx, universe_id, place_id, &file, published).await,
                PlaceCommand::Get {
                    universe_id,
                    place_id,
                } => cmd::place::get(&ctx, universe_id, place_id).await,
                PlaceCommand::History { place_id, limit } => {
                    cmd::place::history(&ctx, place_id, limit).await
                }
            },
            Command::Asset(args) => match args.command {
                AssetCommand::Create {
                    file,
                    display_name,
                    description,
                    user_id,
                    group_id,
                    asset_type,
                } => {
                    cmd::asset::create(
                        &ctx,
                        &file,
                        &display_name,
                        &description,
                        user_id,
                        group_id,
                        asset_type.as_deref(),
                    )
                    .await
                }
                AssetCommand::Update {
                    asset_id,
                    file,
                    asset_type,
                    display_name,
                    description,
                    icon,
                    preview,
                } => {
                    cmd::asset::update(
                        &ctx,
                        asset_id,
                        &file,
                        asset_type.as_deref(),
                        display_name.as_deref(),
                        description.as_deref(),
                        icon.as_deref(),
                        preview.as_deref(),
                    )
                    .await
                }
                AssetCommand::Get {
                    asset_id,
                    read_mask,
                } => cmd::asset::get(&ctx, asset_id, read_mask.as_deref()).await,
                AssetCommand::Versions {
                    asset_id,
                    max_page_size,
                } => cmd::asset::versions(&ctx, asset_id, max_page_size).await,
                AssetCommand::Rollback { asset_id, version } => {
                    cmd::asset::rollback(&ctx, asset_id, version).await
                }
                AssetCommand::Archive { asset_id } => cmd::asset::archive(&ctx, asset_id).await,
                AssetCommand::Operation { operation_id } => {
                    cmd::asset::operation(&ctx, &operation_id).await
                }
            },
            Command::Thumbnail(args) => match args.command {
                ThumbnailCommand::Upload { universe_id, files } => {
                    cmd::thumbnail::upload(&ctx, universe_id, &files).await
                }
                ThumbnailCommand::Status {
                    universe_id,
                    operation_ids,
                } => cmd::thumbnail::status(&ctx, universe_id, &operation_ids).await,
                ThumbnailCommand::List { universe_id } => {
                    cmd::thumbnail::list(&ctx, universe_id).await
                }
                ThumbnailCommand::Delete {
                    universe_id,
                    thumbnail_ids,
                } => cmd::thumbnail::delete(&ctx, universe_id, &thumbnail_ids).await,
            },
            Command::Server(args) => match args.command {
                ServerCommand::List {
                    universe_id,
                    place_id,
                    version,
                } => cmd::server::list(&ctx, universe_id, place_id, version).await,
                ServerCommand::Logs {
                    universe_id,
                    place_id,
                    version,
                    job_id,
                    limit,
                    full,
                } => {
                    cmd::server::logs(&ctx, universe_id, place_id, version, &job_id, limit, full)
                        .await
                }
                ServerCommand::Restart {
                    universe_id,
                    dry_run,
                } => cmd::server::restart(&ctx, universe_id, dry_run).await,
                ServerCommand::RestartForecast { universe_id } => {
                    cmd::server::restart_forecast(&ctx, universe_id).await
                }
            },
            Command::Analytics(args) => match args.command {
                AnalyticsCommand::Metrics { universe_id, query } => {
                    cmd::analytics::metrics(&ctx, universe_id, &query).await
                }
                AnalyticsCommand::Operation {
                    universe_id,
                    kind,
                    operation_id,
                } => cmd::analytics::operation(&ctx, universe_id, &kind, &operation_id).await,
                AnalyticsCommand::Catalog => cmd::analytics::catalog().await,
            },
            Command::Datastore(args) => match args.command {
                DatastoreCommand::Get {
                    universe_id,
                    store,
                    entry,
                    scope,
                } => cmd::datastore::get(&ctx, universe_id, &store, &entry, scope.as_deref()).await,
                DatastoreCommand::Set {
                    universe_id,
                    store,
                    entry,
                    scope,
                    value,
                    create,
                } => {
                    cmd::datastore::set(
                        &ctx,
                        universe_id,
                        &store,
                        &entry,
                        scope.as_deref(),
                        &value,
                        create,
                    )
                    .await
                }
                DatastoreCommand::Delete {
                    universe_id,
                    store,
                    entry,
                    scope,
                } => {
                    cmd::datastore::delete(&ctx, universe_id, &store, &entry, scope.as_deref())
                        .await
                }
                DatastoreCommand::List {
                    universe_id,
                    store,
                    max_page_size,
                } => cmd::datastore::list(&ctx, universe_id, &store, max_page_size).await,
            },
            Command::Gamepass(args) => match args.command {
                GamepassCommand::List { universe_id } => {
                    cmd::monetization::gamepass_list(&ctx, universe_id).await
                }
                GamepassCommand::Create {
                    universe_id,
                    name,
                    description,
                    price,
                    image,
                } => {
                    cmd::monetization::gamepass_create(
                        &ctx,
                        universe_id,
                        &name,
                        description.as_deref(),
                        price,
                        image.as_ref(),
                    )
                    .await
                }
                GamepassCommand::Update {
                    universe_id,
                    game_pass_id,
                    name,
                    description,
                    price,
                    image,
                } => {
                    cmd::monetization::gamepass_update(
                        &ctx,
                        universe_id,
                        game_pass_id,
                        name.as_deref(),
                        description.as_deref(),
                        price,
                        image.as_ref(),
                    )
                    .await
                }
            },
            Command::Devproduct(args) => match args.command {
                DevproductCommand::List { universe_id } => {
                    cmd::monetization::devproduct_list(&ctx, universe_id).await
                }
                DevproductCommand::Create {
                    universe_id,
                    name,
                    description,
                    price,
                    image,
                } => {
                    cmd::monetization::devproduct_create(
                        &ctx,
                        universe_id,
                        &name,
                        description.as_deref(),
                        price,
                        image.as_ref(),
                    )
                    .await
                }
            },
            Command::Universe(args) => match args.command {
                UniverseCommand::Get { universe_id } => cmd::universe::get(&ctx, universe_id).await,
                UniverseCommand::PublishMessage {
                    universe_id,
                    topic,
                    message,
                } => cmd::universe::publish_message(&ctx, universe_id, &topic, &message).await,
            },
        }
    }
}

/// The API catalog. No credentials required.
async fn run_api(args: &ApiArgs) -> Output {
    match &args.command {
        ApiCommand::List {
            implemented,
            query,
            auth,
        } => {
            let mut hits: Vec<&catalog::Endpoint> = match query {
                Some(q) => catalog::search(q),
                None if *implemented => catalog::implemented(),
                None => catalog::ENDPOINTS.iter().collect(),
            };
            // With a query, narrow to implemented here.
            if *implemented && query.is_some() {
                hits.retain(|e| e.implemented);
            }
            if let Some(want) = auth {
                let want = match want.as_str() {
                    "api-key" => catalog::AuthKind::ApiKey,
                    "oauth" => catalog::AuthKind::OAuth,
                    "legacy-cookie" => catalog::AuthKind::LegacyCookie,
                    other => {
                        return failure(
                            "api list",
                            crate::output::ErrorBody {
                                code: "unknown_auth",
                                message: format!("--auth does not accept {other}"),
                                status: None,
                                detail: Some("Use one of: api-key, oauth, legacy-cookie".into()),
                            },
                        );
                    }
                };
                hits.retain(|e| e.auth.contains(&want));
            }
            let impl_count = hits.iter().filter(|e| e.implemented).count();
            Output::success(
                "api list",
                format!("{} endpoints ({impl_count} implemented)", hits.len()),
                Some(serde_json::to_value(&hits).unwrap_or(serde_json::Value::Null)),
            )
            .with_hints(vec![
                "Run `rbxoc api show <id>` for details".into(),
                "Run `rbxoc api mutating` to see mutating operations only".into(),
            ])
        }
        ApiCommand::Show { id } => match catalog::find(id) {
            Some(ep) => Output::success(
                "api show",
                format!("{} {} - {}", ep.method, ep.path, ep.summary),
                serde_json::to_value(ep).ok(),
            ),
            None => {
                let similar = catalog::search(id);
                let mut out = failure(
                    "api show",
                    crate::output::ErrorBody {
                        code: "unknown_endpoint",
                        message: format!("endpoint {id} is not in the catalog"),
                        status: None,
                        detail: None,
                    },
                );
                if !similar.is_empty() {
                    out.hints.push(format!(
                        "Close IDs: {}",
                        similar.iter().map(|e| e.id).collect::<Vec<_>>().join(", ")
                    ));
                }
                out.hints
                    .push("Run `rbxoc api list` for the full catalog".into());
                out
            }
        },
        ApiCommand::Mutating => {
            let hits: Vec<&catalog::Endpoint> = catalog::ENDPOINTS
                .iter()
                .filter(|e| e.mutating && e.implemented)
                .collect();
            Output::success(
                "api mutating",
                format!("{} mutating operations", hits.len()),
                serde_json::to_value(&hits).ok(),
            )
            .with_hints(vec![
                "These change data. Confirm the target IDs before running".into(),
            ])
        }
    }
}

/// Credential inspection, with an optional live probe.
async fn run_auth(
    api_key: Option<&str>,
    api_key_file: Option<&std::path::PathBuf>,
    command: &AuthCommand,
) -> Output {
    let AuthCommand::Check { universe_id } = command;
    let auth = match auth::resolve(api_key, api_key_file) {
        Ok(a) => a,
        Err(e) => {
            return failure("auth check", auth::missing_auth_error(format!("{e:#}")));
        }
    };

    let source = if api_key.is_some() {
        "--api-key"
    } else if std::env::var("RBXOC_API_KEY").is_ok() {
        "RBXOC_API_KEY"
    } else if std::env::var("RBXOC_OAUTH_TOKEN").is_ok() {
        "RBXOC_OAUTH_TOKEN"
    } else {
        "~/.rbxoc/"
    };

    let ctx = match Ctx::new(auth.clone(), 1) {
        Ok(c) => c,
        Err(e) => {
            return failure(
                "auth check",
                crate::output::ErrorBody {
                    code: "client_init_failed",
                    message: format!("{e:#}"),
                    status: None,
                    detail: None,
                },
            );
        }
    };

    match universe_id {
        Some(id) => {
            let path = format!("/cloud/v2/universes/{id}");
            match ctx.get_json(&path).await {
                Ok(_) => Output::success(
                    "auth check",
                    format!("Credentials work ({} / {})", auth.kind(), auth.masked()),
                    Some(serde_json::json!({
                        "authKind": auth.kind(),
                        "source": source,
                        "probe": format!("read universe {id}"),
                    })),
                ),
                Err(err) => failure("auth check", err),
            }
        }
        None => Output::success(
            "auth check",
            format!("Resolved credentials ({} / {})", auth.kind(), auth.masked()),
            Some(serde_json::json!({
                "authKind": auth.kind(),
                "source": source,
                "probe": null,
            })),
        )
        .with_hints(vec![
            "Add --universe-id to make a real API call and verify connectivity".into(),
        ]),
    }
}
