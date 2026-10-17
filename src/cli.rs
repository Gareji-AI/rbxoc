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
use crate::cmd::assets;
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

    /// API key. Prefer RBXOC_API_KEY or the project's .rbxoc/api-key; passing
    /// a key here records it in shell history.
    #[arg(long, global = true, hide_env_values = true)]
    pub api_key: Option<String>,

    /// File containing the API key.
    #[arg(long, global = true, value_name = "PATH")]
    pub api_key_file: Option<std::path::PathBuf>,

    /// Retry count for replay-safe reads (GET/HEAD/OPTIONS/TRACE) after transport,
    /// 429, or 5xx failures. Other methods, including mutations, are sent once.
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

    /// Asset ID registry and code generation
    Assets(AssetsArgs),

    /// Replace thumbnails
    Thumbnail(ThumbnailArgs),

    /// Game servers and logs
    Server(ServerArgs),

    /// Analytics queries
    Analytics(AnalyticsArgs),

    /// Virtual events
    Event(EventArgs),

    /// DataStores
    Datastore(DatastoreArgs),

    /// Ordered DataStores (leaderboards)
    OrderedDatastore(OrderedDatastoreArgs),

    /// Game Passes
    Gamepass(GamepassArgs),

    /// Badges
    Badge(BadgeArgs),

    /// Asset permissions
    Permissions(PermissionsArgs),

    /// Remote config and feature flags
    Config(ConfigArgs),

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
        /// Resolve the asset type and report it without uploading
        #[arg(long)]
        dry_run: bool,
    },
    /// Update an asset's content and metadata
    Update {
        #[arg(long)]
        asset_id: u64,
        /// Model content; omit for a metadata-only update
        #[arg(long, value_name = "PATH")]
        file: Option<std::path::PathBuf>,
        #[arg(long, requires = "file")]
        asset_type: Option<String>,
        #[arg(long)]
        display_name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Existing square Image resource, e.g. assets/123; not a local image or Decal ID
        #[arg(long, value_name = "IMAGE_ASSET_RESOURCE")]
        icon: Option<String>,
        /// Existing Image resource; repeat or separate with commas to replace the preview list
        #[arg(long, value_name = "IMAGE_ASSET_RESOURCE", value_delimiter = ',')]
        preview: Vec<String>,
        /// Resolve the asset type and report it without uploading
        #[arg(long)]
        dry_run: bool,
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
    /// Interpret a Studio Decal.Texture as an Image ID; no automatic cloud lookup
    ImageId {
        #[arg(long)]
        decal_id: u64,
        /// Decal.Texture copied from Studio (rbxassetid://ID or Roblox asset URL)
        #[arg(long)]
        texture: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct AssetsArgs {
    #[command(subcommand)]
    pub command: AssetsCommand,
}

#[derive(Debug, Subcommand)]
pub enum AssetsCommand {
    /// Create a starter manifest in the current directory
    Init {
        /// Creator that owns the assets: `user:ID` or `group:ID`
        #[arg(long)]
        creator: String,
        /// Glob for the source files
        #[arg(long, default_value = "assets/**/*")]
        path: String,
        /// Where to write the generated file
        #[arg(long, default_value = "src/shared/assets")]
        output_path: std::path::PathBuf,
        /// `luau` or `typescript`
        #[arg(long, default_value = "luau")]
        language: String,
    },
    /// Show registry status: registered, missing, and orphaned paths
    Status {
        /// Directory holding the manifest. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        dir: std::path::PathBuf,
    },
    /// Record an uploaded asset ID for a file
    Register {
        /// Path as it appears in the manifest glob, using forward slashes
        #[arg(long)]
        path: String,
        /// Roblox asset ID
        #[arg(long)]
        id: u64,
        /// Explicitly resolved Image ID; --id remains the original Decal ID (offline)
        #[arg(long)]
        image_id: Option<u64>,
        #[arg(long, default_value = ".")]
        dir: std::path::PathBuf,
    },
    /// Forget the ID for a file
    Unregister {
        #[arg(long)]
        path: String,
        #[arg(long, default_value = ".")]
        dir: std::path::PathBuf,
        /// Forget a pending upload only after verifying no asset was created
        #[arg(long)]
        discard_pending: bool,
    },
    /// Emit Luau or TypeScript that exposes the IDs by name
    Codegen {
        #[arg(long, default_value = ".")]
        dir: std::path::PathBuf,
        /// Print the source instead of writing the file
        #[arg(long)]
        stdout: bool,
        /// Fail when any manifest path lacks an ID
        #[arg(long)]
        check: bool,
    },
    /// Upload missing files, refresh the lockfile, and run codegen
    Sync {
        #[arg(long, default_value = ".")]
        dir: std::path::PathBuf,
        /// Exact normalized manifest/lock key to sync; repeat for multiple files
        #[arg(long = "path")]
        paths: Vec<String>,
        /// Update only selected changed, receipt-backed Models, preserving IDs
        #[arg(long, requires = "paths")]
        update: bool,
        /// Registry/freshness check only, not full upload preflight (no uploads)
        #[arg(long)]
        dry_run: bool,
    },
    /// Attach a known operation to an uncertain upload receipt (offline)
    Recover {
        #[arg(long, default_value = ".")]
        dir: std::path::PathBuf,
        #[arg(long)]
        path: String,
        #[arg(long)]
        operation_id: String,
    },
    /// Write preprocessed files (SVG rasterize, alpha bleed) to a folder
    Preprocess {
        #[arg(long, default_value = ".")]
        dir: std::path::PathBuf,
        /// Output directory (mirrors manifest paths)
        #[arg(long, default_value = ".rbxoc-debug")]
        out: std::path::PathBuf,
    },
    /// Copy IDs from an Asphalt lockfile (hash keys) into rbxoc.assets.lock.toml
    ImportAsphaltLock {
        #[arg(long, default_value = ".")]
        dir: std::path::PathBuf,
        #[arg(long)]
        asphalt_lock: std::path::PathBuf,
        /// Asphalt input name (e.g. Assets, CharmAssets)
        #[arg(long)]
        input: String,
        /// Write rbxoc.assets.lock.toml after import
        #[arg(long)]
        write: bool,
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
    /// Manage metric alert configurations
    Alerts {
        #[command(subcommand)]
        command: AlertCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AlertCommand {
    /// List alert configurations
    List {
        #[arg(long)]
        universe_id: u64,
        /// Filter by firing status
        #[arg(long)]
        firing_status: Option<String>,
    },
    /// Create an alert from a JSON document
    Create {
        #[arg(long)]
        universe_id: u64,
        /// JSON document; the alert schema is provider-defined
        #[arg(long)]
        document: String,
    },
    /// Update an alert from a JSON document
    Update {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        alert_id: String,
        #[arg(long)]
        document: String,
    },
    /// Delete an alert
    Delete {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        alert_id: String,
    },
    /// List alert incidents in a time window
    Incidents {
        #[arg(long)]
        universe_id: u64,
        /// RFC3339 start time
        #[arg(long)]
        start_time: String,
        /// RFC3339 end time
        #[arg(long)]
        end_time: String,
    },
}

#[derive(Debug, Args)]
pub struct EventArgs {
    #[command(subcommand)]
    pub command: EventCommand,
}

#[derive(Debug, Subcommand)]
pub enum EventCommand {
    /// List game events under a universe
    List {
        #[arg(long)]
        universe_id: u64,
        #[arg(long, default_value_t = 25)]
        page_size: u32,
    },
    /// Get one game event
    Get {
        #[arg(long)]
        event_id: u64,
    },
    /// Create a game event from a JSON document
    Create {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        document: String,
    },
    /// Update a game event from a JSON document
    Update {
        #[arg(long)]
        event_id: u64,
        #[arg(long)]
        document: String,
    },
    /// Delete a game event
    Delete {
        #[arg(long)]
        event_id: u64,
    },
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
        /// JSON value (object, array, string, number, boolean, or null)
        #[arg(long)]
        value: String,
        /// Create if missing; without this flag, update existing entries only
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
pub struct OrderedDatastoreArgs {
    #[command(subcommand)]
    pub command: OrderedDatastoreCommand,
}

#[derive(Debug, Subcommand)]
pub enum OrderedDatastoreCommand {
    /// List entries, highest value first by default
    List {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        store: String,
        /// Scope segment. Required for ordered DataStores.
        #[arg(long, default_value = "global")]
        scope: String,
        /// Sort ascending instead of descending
        #[arg(long)]
        ascending: bool,
        #[arg(long, default_value_t = 50)]
        page_size: u32,
    },
    Get {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        store: String,
        #[arg(long, default_value = "global")]
        scope: String,
        #[arg(long)]
        entry: String,
    },
    Set {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        store: String,
        #[arg(long, default_value = "global")]
        scope: String,
        #[arg(long)]
        entry: String,
        /// Integer value for the entry
        #[arg(long, allow_hyphen_values = true)]
        value: i64,
        /// Create if missing; without this flag, update existing entries only
        #[arg(long)]
        create: bool,
    },
    Delete {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        store: String,
        #[arg(long, default_value = "global")]
        scope: String,
        #[arg(long)]
        entry: String,
    },
    /// Add to an entry's value atomically
    Increment {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        store: String,
        #[arg(long, default_value = "global")]
        scope: String,
        #[arg(long)]
        entry: String,
        /// Amount to add. Negative values subtract.
        #[arg(long, allow_hyphen_values = true)]
        amount: i64,
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
pub struct BadgeArgs {
    #[command(subcommand)]
    pub command: BadgeCommand,
}

#[derive(Debug, Subcommand)]
pub enum BadgeCommand {
    /// Create a badge. Spends Robux.
    Create {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: String,
        /// Who pays the creation fee: `user` or `group`
        #[arg(long)]
        payment_source: Option<String>,
    },
    /// Update a badge
    Update {
        #[arg(long)]
        badge_id: u64,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Enable or disable the badge
        #[arg(long)]
        enabled: Option<bool>,
    },
}

#[derive(Debug, Args)]
pub struct PermissionsArgs {
    #[command(subcommand)]
    pub command: PermissionsCommand,
}

#[derive(Debug, Subcommand)]
pub enum PermissionsCommand {
    /// Grant a user or group access to assets
    Grant {
        /// Asset ID. Repeat for more than one.
        #[arg(long = "asset-id", required = true)]
        asset_ids: Vec<u64>,
        /// `user` or `group`
        #[arg(long, default_value = "user")]
        subject_type: String,
        #[arg(long)]
        subject_id: u64,
        /// `use` to load the asset, `edit` to modify it
        #[arg(long, default_value = "use")]
        action: String,
    },
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Read the published config values
    Get {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        repository: String,
    },
    /// Read the current draft
    Draft {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        repository: String,
    },
    /// Patch the draft with a JSON document
    PatchDraft {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        repository: String,
        /// JSON document
        #[arg(long)]
        document: String,
    },
    /// Replace the whole draft with a JSON document
    Overwrite {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        repository: String,
        /// JSON document
        #[arg(long)]
        document: String,
    },
    /// Publish the draft so the game reads it
    Publish {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        repository: String,
    },
    /// List revision history
    Revisions {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        repository: String,
    },
    /// Stage a revert to a previous revision
    Restore {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        repository: String,
        #[arg(long)]
        revision_id: String,
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

        // Asset registry commands are local-only except `assets sync` (upload path).
        if let Command::Assets(args) = &self.command {
            if let AssetsCommand::Sync {
                dir,
                paths,
                dry_run,
                update,
            } = &args.command
            {
                if *update {
                    // Eligibility and unsupported content fail locally, even
                    // without credentials, before constructing an HTTP context.
                    let plan = cmd::assets_sync::preview_update(dir, paths).await;
                    if !plan.ok || *dry_run {
                        return plan;
                    }
                } else if *dry_run {
                    return cmd::assets_sync::run_dry_run(dir, paths);
                }
            }
            if !matches!(args.command, AssetsCommand::Sync { .. }) {
                return run_assets(&args.command).await;
            }
        }

        if let Command::Auth(AuthArgs { command }) = &self.command {
            return run_auth(self.api_key.as_deref(), self.api_key_file.as_ref(), command).await;
        }

        // `asset create --dry-run` only inspects a local file, so it must work
        // without credentials. Report inference and stop.
        if let Command::Asset(AssetArgs { command }) = &self.command {
            match command {
                AssetCommand::ImageId { decal_id, texture } => {
                    return cmd::asset::image_id(*decal_id, texture.as_deref());
                }
                AssetCommand::Create {
                    file,
                    display_name,
                    description,
                    user_id,
                    group_id,
                    asset_type,
                    dry_run: true,
                } => {
                    return cmd::asset::preview_create(
                        file,
                        display_name,
                        description,
                        *user_id,
                        *group_id,
                        asset_type.as_deref(),
                    )
                    .await;
                }
                AssetCommand::Update {
                    asset_id,
                    file,
                    asset_type,
                    display_name,
                    description,
                    icon,
                    preview,
                    dry_run,
                } => {
                    let plan = cmd::asset::preview_update(
                        file.as_deref(),
                        *asset_id,
                        asset_type.as_deref(),
                        display_name.as_deref(),
                        description.as_deref(),
                        icon.as_deref(),
                        preview,
                    )
                    .await;
                    if !plan.ok || *dry_run {
                        return plan;
                    }
                }
                _ => {}
            }
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

                        refs: None,
                    },
                );
            }
        };

        match self.command {
            Command::Api(_) | Command::Auth(_) => unreachable!("handled above"),
            Command::Assets(args) => match args.command {
                AssetsCommand::Sync {
                    dir,
                    paths,
                    dry_run: false,
                    update,
                } => {
                    if update {
                        cmd::assets_sync::run_update(&ctx, &dir, &paths).await
                    } else {
                        cmd::assets_sync::run(&ctx, &dir, &paths).await
                    }
                }
                _ => unreachable!("handled above"),
            },
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
                AssetCommand::ImageId { .. } => unreachable!("handled without credentials"),
                AssetCommand::Create {
                    file,
                    display_name,
                    description,
                    user_id,
                    group_id,
                    asset_type,
                    dry_run: _,
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
                    dry_run: _,
                } => {
                    cmd::asset::update(
                        &ctx,
                        asset_id,
                        file.as_deref(),
                        asset_type.as_deref(),
                        display_name.as_deref(),
                        description.as_deref(),
                        icon.as_deref(),
                        &preview,
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
                AnalyticsCommand::Alerts { command } => match command {
                    AlertCommand::List {
                        universe_id,
                        firing_status,
                    } => {
                        cmd::alerts::list(&ctx, &universe_id.to_string(), firing_status.as_deref())
                            .await
                    }
                    AlertCommand::Create {
                        universe_id,
                        document,
                    } => cmd::alerts::create(&ctx, &universe_id.to_string(), &document).await,
                    AlertCommand::Update {
                        universe_id,
                        alert_id,
                        document,
                    } => {
                        cmd::alerts::update(&ctx, &universe_id.to_string(), &alert_id, &document)
                            .await
                    }
                    AlertCommand::Delete {
                        universe_id,
                        alert_id,
                    } => cmd::alerts::delete(&ctx, &universe_id.to_string(), &alert_id).await,
                    AlertCommand::Incidents {
                        universe_id,
                        start_time,
                        end_time,
                    } => {
                        cmd::alerts::incidents(
                            &ctx,
                            &universe_id.to_string(),
                            &start_time,
                            &end_time,
                        )
                        .await
                    }
                },
            },
            Command::Event(args) => match args.command {
                EventCommand::List {
                    universe_id,
                    page_size,
                } => cmd::event::list(&ctx, universe_id, page_size).await,
                EventCommand::Get { event_id } => cmd::event::get(&ctx, event_id).await,
                EventCommand::Create {
                    universe_id,
                    document,
                } => cmd::event::create(&ctx, universe_id, &document).await,
                EventCommand::Update { event_id, document } => {
                    cmd::event::update(&ctx, event_id, &document).await
                }
                EventCommand::Delete { event_id } => cmd::event::delete(&ctx, event_id).await,
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
            Command::OrderedDatastore(args) => match args.command {
                OrderedDatastoreCommand::List {
                    universe_id,
                    store,
                    scope,
                    ascending,
                    page_size,
                } => {
                    cmd::ordered_datastore::list(
                        &ctx,
                        universe_id,
                        &store,
                        &scope,
                        ascending,
                        page_size,
                    )
                    .await
                }
                OrderedDatastoreCommand::Get {
                    universe_id,
                    store,
                    scope,
                    entry,
                } => cmd::ordered_datastore::get(&ctx, universe_id, &store, &scope, &entry).await,
                OrderedDatastoreCommand::Set {
                    universe_id,
                    store,
                    scope,
                    entry,
                    value,
                    create,
                } => {
                    cmd::ordered_datastore::set(
                        &ctx,
                        universe_id,
                        &store,
                        &scope,
                        &entry,
                        value,
                        create,
                    )
                    .await
                }
                OrderedDatastoreCommand::Delete {
                    universe_id,
                    store,
                    scope,
                    entry,
                } => {
                    cmd::ordered_datastore::delete(&ctx, universe_id, &store, &scope, &entry).await
                }
                OrderedDatastoreCommand::Increment {
                    universe_id,
                    store,
                    scope,
                    entry,
                    amount,
                } => {
                    cmd::ordered_datastore::increment(
                        &ctx,
                        universe_id,
                        &store,
                        &scope,
                        &entry,
                        amount,
                    )
                    .await
                }
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
            Command::Badge(args) => match args.command {
                BadgeCommand::Create {
                    universe_id,
                    name,
                    description,
                    payment_source,
                } => {
                    cmd::badge::create(
                        &ctx,
                        universe_id,
                        &name,
                        &description,
                        payment_source.as_deref(),
                    )
                    .await
                }
                BadgeCommand::Update {
                    badge_id,
                    name,
                    description,
                    enabled,
                } => {
                    cmd::badge::update(
                        &ctx,
                        badge_id,
                        name.as_deref(),
                        description.as_deref(),
                        enabled,
                    )
                    .await
                }
            },
            Command::Permissions(args) => match args.command {
                PermissionsCommand::Grant {
                    asset_ids,
                    subject_type,
                    subject_id,
                    action,
                } => {
                    cmd::permissions::grant(&ctx, &asset_ids, &subject_type, subject_id, &action)
                        .await
                }
            },
            Command::Config(args) => match args.command {
                ConfigCommand::Get {
                    universe_id,
                    repository,
                } => cmd::config::get(&ctx, universe_id, &repository).await,
                ConfigCommand::Draft {
                    universe_id,
                    repository,
                } => cmd::config::draft(&ctx, universe_id, &repository).await,
                ConfigCommand::PatchDraft {
                    universe_id,
                    repository,
                    document,
                } => cmd::config::patch_draft(&ctx, universe_id, &repository, &document).await,
                ConfigCommand::Overwrite {
                    universe_id,
                    repository,
                    document,
                } => cmd::config::overwrite(&ctx, universe_id, &repository, &document).await,
                ConfigCommand::Publish {
                    universe_id,
                    repository,
                } => cmd::config::publish(&ctx, universe_id, &repository).await,
                ConfigCommand::Revisions {
                    universe_id,
                    repository,
                } => cmd::config::revisions(&ctx, universe_id, &repository).await,
                ConfigCommand::Restore {
                    universe_id,
                    repository,
                    revision_id,
                } => cmd::config::restore(&ctx, universe_id, &repository, &revision_id).await,
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

                                refs: None,
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

                        refs: None,
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
    let auth::ResolvedAuth { auth, source } = match auth::resolve_with_source(api_key, api_key_file)
    {
        Ok(a) => a,
        Err(e) => {
            return failure("auth check", auth::missing_auth_error(format!("{e:#}")));
        }
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

                    refs: None,
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

/// The asset ID registry. Works on local files only, so it needs no credentials.
async fn run_assets(command: &AssetsCommand) -> Output {
    match command {
        AssetsCommand::Init {
            creator,
            path,
            output_path,
            language,
        } => init_manifest(creator, path, output_path, language),
        AssetsCommand::Status { dir } => registry_status(dir),
        AssetsCommand::Register {
            path,
            id,
            image_id,
            dir,
        } => register_asset(dir, path, *id, *image_id),
        AssetsCommand::Unregister {
            path,
            dir,
            discard_pending,
        } => unregister_asset(dir, path, *discard_pending),
        AssetsCommand::Codegen { dir, stdout, check } => run_codegen(dir, *stdout, *check),
        AssetsCommand::Sync { .. } => unreachable!("handled in Cli::run with credentials"),
        AssetsCommand::Recover {
            dir,
            path,
            operation_id,
        } => cmd::assets_sync::recover_operation(dir, path, operation_id),
        AssetsCommand::Preprocess { dir, out } => cmd::assets_preprocess::run(dir, out),
        AssetsCommand::ImportAsphaltLock {
            dir,
            asphalt_lock,
            input,
            write,
        } => import_asphalt_lock(dir, asphalt_lock, input, *write),
    }
}

/// Parse `user:123` or `group:456`.
fn parse_creator(spec: &str) -> Result<assets::Creator, String> {
    let (kind, id) = spec
        .split_once(':')
        .ok_or_else(|| format!("--creator needs the form user:ID or group:ID, got {spec}"))?;
    if !matches!(kind, "user" | "group") {
        return Err(format!("--creator type must be user or group, got {kind}"));
    }
    let id: u64 = id
        .trim()
        .parse()
        .map_err(|_| format!("--creator id must be a number, got {id}"))?;
    Ok(assets::Creator {
        kind: kind.to_string(),
        id,
    })
}

fn init_manifest(
    creator: &str,
    glob_path: &str,
    output_path: &std::path::Path,
    language: &str,
) -> Output {
    const COMMAND: &str = "assets init";
    let creator = match parse_creator(creator) {
        Ok(c) => c,
        Err(msg) => {
            return failure(
                COMMAND,
                crate::output::ErrorBody {
                    code: "invalid_creator",
                    message: msg,
                    status: None,
                    detail: None,

                    refs: None,
                },
            );
        }
    };
    if !matches!(language, "luau" | "typescript") {
        return failure(
            COMMAND,
            crate::output::ErrorBody {
                code: "invalid_language",
                message: format!("--language must be luau or typescript, got {language}"),
                status: None,
                detail: None,

                refs: None,
            },
        );
    }

    let dir = std::path::Path::new(".");
    let target = dir.join(assets::MANIFEST_NAME);
    if target.exists() {
        return failure(
            COMMAND,
            crate::output::ErrorBody {
                code: "manifest_exists",
                message: format!("{} already exists", assets::MANIFEST_NAME),
                status: None,
                detail: Some("Edit it directly instead of re-initializing".into()),

                refs: None,
            },
        );
    }

    let manifest = assets::Manifest {
        creator,
        codegen: assets::CodegenConfig {
            language: language.to_string(),
            style: "nested".into(),
            output_path: output_path.to_path_buf(),
            variable: "Assets".into(),
            output_basename: None,
            generator: "rbxoc".into(),
            strip_extensions: false,
            content: false,
            image_ids: false,
            bleed: true,
        },
        inputs: [(
            "assets".to_string(),
            assets::Inputs {
                path: glob_path.to_string(),
                path_key_from: None,
                bleed: None,
            },
        )]
        .into_iter()
        .collect(),
    };

    let body = match toml::to_string_pretty(&manifest) {
        Ok(b) => b,
        Err(e) => {
            return failure(
                COMMAND,
                crate::output::ErrorBody {
                    code: "serialize_failed",
                    message: format!("cannot serialize the manifest: {e}"),
                    status: None,
                    detail: None,

                    refs: None,
                },
            );
        }
    };
    let header = "# Asset ID registry for rbxoc.\n\
                  # Commit this file. The lockfile holds the uploaded IDs.\n\n";
    if let Err(e) = std::fs::write(&target, format!("{header}{body}")) {
        return failure(
            COMMAND,
            crate::output::ErrorBody {
                code: "write_failed",
                message: format!("cannot write {}: {e}", target.display()),
                status: None,
                detail: None,

                refs: None,
            },
        );
    }

    Output::success(
        COMMAND,
        format!("Created {}", assets::MANIFEST_NAME),
        Some(serde_json::json!({
            "manifest": assets::MANIFEST_NAME,
            "inputPath": glob_path,
            "outputPath": output_path,
            "language": language,
        })),
    )
    .with_hints(vec![
        "Upload each file with `rbxoc asset create`, then record the ID with `rbxoc assets register`".into(),
        "Run `rbxoc assets codegen` to emit the reference table".into(),
    ])
}

/// Shared load of manifest and lockfile, returning a failure on error.
/// The error is boxed because `Output` is large and this path is cold.
fn load_registry(
    dir: &std::path::Path,
    command: &str,
) -> Result<(assets::Manifest, assets::Lockfile), Box<Output>> {
    let manifest =
        assets::load_manifest(dir).map_err(|e| Box::new(registry_failure(command, e)))?;
    let lock = assets::load_lockfile(dir).map_err(|e| Box::new(registry_failure(command, e)))?;
    Ok((manifest, lock))
}

fn registry_failure(command: &str, err: anyhow::Error) -> Output {
    failure(
        command,
        crate::output::ErrorBody {
            code: "registry_error",
            message: format!("{err:#}"),
            status: None,
            detail: None,

            refs: None,
        },
    )
}

fn registry_status(dir: &std::path::Path) -> Output {
    const COMMAND: &str = "assets status";
    let (manifest, lock) = match load_registry(dir, COMMAND) {
        Ok(v) => v,
        Err(out) => return *out,
    };
    let sources = match assets::collect_sources(dir, &manifest) {
        Ok(sources) => sources,
        Err(e) => return registry_failure(COMMAND, e),
    };
    let paths: Vec<_> = sources.keys().cloned().collect();
    let freshness = match assets::freshness(&sources, &paths, &manifest, &lock) {
        Ok(state) => state,
        Err(e) => return registry_failure(COMMAND, e),
    };
    let missing = assets::missing_ids(&paths, &lock);
    let orphaned = assets::orphaned_ids(&paths, &lock);
    let registered = paths.len() - missing.len();

    let mut out = Output::success(
        COMMAND,
        format!(
            "{registered}/{} files registered, {} missing, {} orphaned",
            paths.len(),
            missing.len(),
            orphaned.len()
        ),
        Some(serde_json::json!({
            "total": paths.len(),
            "registered": registered,
            "missing": missing,
            "orphaned": orphaned,
            "current": freshness.current,
            "changed": freshness.changed,
            "unverified": freshness.unverified,
            "pending": lock.pending.keys().collect::<Vec<_>>(),
            "imageIds": lock.image_ids,
        })),
    );
    if !missing.is_empty() {
        out = out.with_hints(vec![
            "Upload the missing files with `rbxoc asset create`, then `rbxoc assets register`"
                .into(),
        ]);
    }
    if !orphaned.is_empty() {
        out = out.with_hints(vec![
            "Orphaned IDs no longer match the manifest. Remove them with `rbxoc assets unregister`"
                .into(),
        ]);
    }
    if !freshness.changed.is_empty() {
        out.hints.push("Changed content is not published automatically; IDs are preserved. Explicitly update supported Model assets with `asset update`, or create/register a replacement.".into());
    }
    if !freshness.unverified.is_empty() {
        out.hints.push("Unverified entries have no matching publication fingerprint; registration alone does not prove remote freshness.".into());
    }
    out
}

fn registry_path_key(path: &str) -> String {
    path.replace('\\', "/")
}

enum RegistryMutation {
    Register { id: u64, image_id: Option<u64> },
    Unregister { discard_pending: bool },
}

fn mutate_registry(
    dir: &std::path::Path,
    path: &str,
    command: &str,
    mutation: RegistryMutation,
) -> Output {
    let _guard = match assets::registry_guard(dir) {
        Ok(guard) => guard,
        Err(e) => return registry_failure(command, e),
    };
    let (_, mut lock) = match load_registry(dir, command) {
        Ok(v) => v,
        Err(out) => return *out,
    };
    let key = registry_path_key(path);
    match mutation {
        RegistryMutation::Register { id, image_id } => {
            if id == 0 || image_id == Some(0) {
                return registry_failure(
                    command,
                    anyhow::anyhow!(
                        "asset and Image IDs must be positive; original receipt was not modified"
                    ),
                );
            }
            let previous = lock.assets.insert(key.clone(), id);
            lock.publications.remove(&key);
            lock.pending.remove(&key);
            if let Some(image_id) = image_id {
                lock.image_ids.insert(
                    key.clone(),
                    assets::ImageReference {
                        decal_id: id,
                        image_id,
                    },
                );
            } else if previous != Some(id) {
                lock.image_ids.remove(&key);
            }
            match assets::save_lockfile(dir, &lock) {
                Ok(_) => {
                    let summary = match previous {
                        Some(old) if old != id => format!("Updated {key}: {old} -> {id}"),
                        Some(_) => format!("{key} already mapped to {id}"),
                        None => format!("Registered {key} -> {id}"),
                    };
                    Output::success(
                        command,
                        summary,
                        Some(
                            serde_json::json!({ "path": key, "assetId": id, "previous": previous,
                                "decalId": image_id.map(|_| id), "imageId": image_id, "networkValidated": false }),
                        ),
                    )
                    .with_hints(vec!["Registration is offline; no asset-type or Decal/Image association was checked. ImageLabel/ImageButton need an Image ID, not an unresolved Decal ID. Set codegen.image_ids = true only after registering resolved --image-id values.".into()])
                }
                Err(e) => registry_failure(command, e),
            }
        }
        RegistryMutation::Unregister { discard_pending } => {
            if lock.pending.contains_key(&key) && !discard_pending {
                return failure(command, crate::output::ErrorBody {
                    code: "publication_pending", message: format!("{key}: refusing to forget a pending upload; inspect its outcome first"),
                    status: None, detail: Some("Use --discard-pending only after verifying no asset was created; discarding an uncertain receipt can cause duplicates.".into()), refs: None,
                });
            }
            lock.publications.remove(&key);
            lock.image_ids.remove(&key);
            let removed = lock.assets.remove(&key);
            let pending = lock.pending.remove(&key);
            if removed.is_some() || pending.is_some() {
                match assets::save_lockfile(dir, &lock) {
                    Ok(_) => Output::success(
                        command,
                        format!("Removed {key}"),
                        Some(serde_json::json!({ "path": key, "removedId": removed })),
                    ),
                    Err(e) => registry_failure(command, e),
                }
            } else {
                failure(
                    command,
                    crate::output::ErrorBody {
                        code: "not_registered",
                        message: format!("{key} is not in the lockfile"),
                        status: None,
                        detail: None,

                        refs: None,
                    },
                )
            }
        }
    }
}

fn register_asset(dir: &std::path::Path, path: &str, id: u64, image_id: Option<u64>) -> Output {
    mutate_registry(
        dir,
        path,
        "assets register",
        RegistryMutation::Register { id, image_id },
    )
}

fn unregister_asset(dir: &std::path::Path, path: &str, discard_pending: bool) -> Output {
    mutate_registry(
        dir,
        path,
        "assets unregister",
        RegistryMutation::Unregister { discard_pending },
    )
}

fn run_codegen(dir: &std::path::Path, to_stdout: bool, check: bool) -> Output {
    const COMMAND: &str = "assets codegen";
    let (manifest, lock) = match load_registry(dir, COMMAND) {
        Ok(v) => v,
        Err(out) => return *out,
    };
    let paths = match assets::collect_paths(dir, &manifest) {
        Ok(p) => p,
        Err(e) => return registry_failure(COMMAND, e),
    };
    let missing = assets::missing_ids(&paths, &lock);

    if check && !missing.is_empty() {
        let mut out = failure(
            COMMAND,
            crate::output::ErrorBody {
                code: "missing_ids",
                message: format!("{} files have no asset ID yet", missing.len()),
                status: None,
                detail: Some(missing.join(", ")),

                refs: None,
            },
        );
        out.hints
            .push("Upload and register them, then run this command again".into());
        return out;
    }

    let source = match assets::render(&manifest, &lock) {
        Ok(source) => source,
        Err(e) => return registry_failure(COMMAND, e),
    };

    if to_stdout {
        return Output::success(
            COMMAND,
            format!("Generated source for {} assets", lock.assets.len()),
            Some(serde_json::json!({ "source": source, "written": null })),
        );
    }

    let out_file = match assets::write_generated(dir, &manifest, &lock) {
        Ok(p) => p,
        Err(e) => return registry_failure(COMMAND, e),
    };

    let mut out = Output::success(
        COMMAND,
        format!(
            "Wrote {} ({} assets)",
            out_file.display(),
            lock.assets.len()
        ),
        Some(serde_json::json!({
            "output": out_file.to_string_lossy(),
            "assetCount": lock.assets.len(),
            "missing": missing,
        })),
    );
    if !missing.is_empty() {
        out = out.with_hints(vec![format!(
            "{} files still have no ID; the generated table omits them",
            missing.len()
        )]);
    }
    out
}

fn import_asphalt_lock(
    dir: &std::path::Path,
    asphalt_lock: &std::path::Path,
    input: &str,
    write: bool,
) -> Output {
    const COMMAND: &str = "assets import-asphalt-lock";
    let _guard = if write {
        match assets::registry_guard(dir) {
            Ok(guard) => Some(guard),
            Err(e) => return registry_failure(COMMAND, e),
        }
    } else {
        None
    };
    match assets::import_asphalt_lock(dir, asphalt_lock, input) {
        Ok((imported, lock)) => {
            let lock_path = if write {
                match assets::save_lockfile(dir, &lock) {
                    Ok(p) => Some(p),
                    Err(e) => return registry_failure(COMMAND, e),
                }
            } else {
                None
            };
            Output::success(
                COMMAND,
                format!("Matched {imported} paths from Asphalt input {input}"),
                Some(serde_json::json!({
                    "imported": imported,
                    "registered": lock.assets.len(),
                    "lockfile": lock_path.map(|p| p.to_string_lossy().to_string()),
                })),
            )
            .with_hints(if write {
                vec!["Run `rbxoc assets sync` to codegen (uploads only missing paths)".into()]
            } else {
                vec!["Re-run with --write to save rbxoc.assets.lock.toml".into()]
            })
        }
        Err(e) => registry_failure(COMMAND, e),
    }
}

#[cfg(test)]
mod asset_workflow_tests {
    use super::*;

    #[tokio::test]
    async fn image_id_command_is_explicitly_offline() {
        let cli = Cli::try_parse_from([
            "rbxoc",
            "asset",
            "image-id",
            "--decal-id",
            "123",
            "--texture",
            "rbxassetid://456",
        ])
        .unwrap();
        let out = cli.run().await;
        assert!(out.ok);
        assert_eq!(out.data.unwrap()["imageId"], 456);
        let cli = Cli::try_parse_from(["rbxoc", "asset", "image-id", "--decal-id", "123"]).unwrap();
        assert_eq!(
            cli.run().await.error.unwrap().code,
            "image_resolution_unavailable"
        );
    }

    #[test]
    fn register_keeps_distinct_ids_and_invalid_input_preserves_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = r#"[creator]
type = "group"
id = 7
[codegen]
output_path = "generated"
"#;
        std::fs::write(dir.path().join(assets::MANIFEST_NAME), manifest).unwrap();
        assert!(register_asset(dir.path(), "ui/icon.png", 123, Some(456)).ok);
        let lock = assets::load_lockfile(dir.path()).unwrap();
        assert_eq!(lock.assets["ui/icon.png"], 123);
        assert_eq!(lock.image_ids["ui/icon.png"].image_id, 456);
        let before = std::fs::read(dir.path().join(assets::LOCKFILE_NAME)).unwrap();
        assert!(!register_asset(dir.path(), "ui/icon.png", 123, Some(0)).ok);
        assert_eq!(
            std::fs::read(dir.path().join(assets::LOCKFILE_NAME)).unwrap(),
            before
        );
        assert!(register_asset(dir.path(), "ui/icon.png", 999, None).ok);
        assert!(
            !assets::load_lockfile(dir.path())
                .unwrap()
                .image_ids
                .contains_key("ui/icon.png")
        );
    }

    #[test]
    fn repeatable_sync_paths_are_preserved_by_cli() {
        let cli = Cli::try_parse_from([
            "rbxoc",
            "assets",
            "sync",
            "--path",
            "ui/a.png",
            "--path",
            "ui/b.svg",
            "--dry-run",
        ])
        .unwrap();
        match cli.command {
            Command::Assets(AssetsArgs {
                command: AssetsCommand::Sync { paths, dry_run, .. },
            }) => {
                assert_eq!(paths, vec!["ui/a.png", "ui/b.svg"]);
                assert!(dry_run);
            }
            _ => panic!("expected scoped sync"),
        }
    }

    #[test]
    fn unregister_requires_explicit_pending_discard() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(assets::MANIFEST_NAME),
            "[creator]\ntype='group'\nid=7\n[codegen]\noutput_path='generated'\n",
        )
        .unwrap();
        let mut lock = assets::Lockfile::new();
        lock.pending.insert(
            "a.png".into(),
            assets::PendingUpload {
                fingerprint: "test-fingerprint".into(),
                creator: assets::Creator {
                    kind: "group".into(),
                    id: 7,
                },
                asset_type: "Decal".into(),
                display_name: None,
                requested_at: None,
                target_asset_id: None,
                operation_id: Some("op1".into()),
            },
        );
        assets::save_lockfile(dir.path(), &lock).unwrap();
        assert_eq!(
            unregister_asset(dir.path(), "a.png", false)
                .error
                .unwrap()
                .code,
            "publication_pending"
        );
        assert!(
            assets::load_lockfile(dir.path())
                .unwrap()
                .pending
                .contains_key("a.png")
        );
        assert!(unregister_asset(dir.path(), "a.png", true).ok);
        assert!(
            assets::load_lockfile(dir.path())
                .unwrap()
                .pending
                .is_empty()
        );
    }
}
