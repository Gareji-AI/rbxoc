//! コマンド定義と実行。
//!
//! エージェント向け CLI の原則:
//! - すべてのコマンドが同じ封筒（`Output`）を返す
//! - 発見は `rbxoc api list` / `rbxoc api show <id>` の自己記述カタログから
//! - 失敗には必ず `hints` が付く
//! - 秘密値は引数より環境変数とファイルを優先する

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
    long_about = "Roblox Open Cloud を操作する CLI。\n\
                  すべてのコマンドが JSON の封筒を返し、失敗時は hints に\n\
                  次の行動を含めます。何ができるかは `rbxoc api list` で\n\
                  決定論的に発見できます。"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// 出力形式。既定は json。RBXOC_FORMAT でも指定できる。
    #[arg(long, value_enum, global = true)]
    pub format: Option<Format>,

    /// API key。省略時は RBXOC_API_KEY か ~/.rbxoc/api-key を読む。
    #[arg(long, global = true, env = "RBXOC_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,

    /// API key を書いたファイル。
    #[arg(long, global = true, value_name = "PATH")]
    pub api_key_file: Option<std::path::PathBuf>,

    /// 429 / 5xx の自動再試行回数。
    #[arg(long, global = true, default_value_t = 2)]
    pub retry: u32,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 認証の状態を確認する
    Auth(AuthArgs),

    /// API カタログを読む（何ができるかを発見する）
    Api(ApiArgs),

    /// place を公開・取得する
    Place(PlaceArgs),

    /// アセットを扱う
    Asset(AssetArgs),

    /// サムネイルを差し替える
    Thumbnail(ThumbnailArgs),

    /// ゲームサーバーとログ
    Server(ServerArgs),

    /// 分析クエリ
    Analytics(AnalyticsArgs),

    /// DataStore
    Datastore(DatastoreArgs),

    /// 課金商品
    Gamepass(GamepassArgs),

    /// Developer Product
    Devproduct(DevproductArgs),

    /// universe
    Universe(UniverseArgs),
}

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// 認証情報の探索結果と疎通を確認する
    Check {
        /// 疎通確認に使う universe ID。省略時は存在確認のみ。
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
    /// 全エンドポイントを一覧する
    List {
        /// 実装済みだけを出す
        #[arg(long)]
        implemented: bool,
        /// 部分一致で絞る
        #[arg(long)]
        query: Option<String>,
        /// 認証方式で絞る（api-key / oauth）
        #[arg(long)]
        auth: Option<String>,
    },
    /// 1 つのエンドポイントの詳細を見る
    Show {
        /// エンドポイント ID（例: place.publish）
        id: String,
    },
    /// 破壊的操作の一覧。実行前に確認するため。
    Mutating,
}

#[derive(Debug, Args)]
pub struct PlaceArgs {
    #[command(subcommand)]
    pub command: PlaceCommand,
}

#[derive(Debug, Subcommand)]
pub enum PlaceCommand {
    /// RBXL / RBXLX を保存または公開する
    Publish {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        place_id: u64,
        /// 送るファイル
        #[arg(long, value_name = "PATH")]
        file: std::path::PathBuf,
        /// 公開まで進める。省略時は保存のみ。
        #[arg(long)]
        published: bool,
    },
    /// place のメタデータを取得する
    Get {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        place_id: u64,
    },
    /// place のバージョン履歴を取得する
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
    /// アセットをアップロードする
    Create {
        #[arg(long, value_name = "PATH")]
        file: std::path::PathBuf,
        #[arg(long)]
        display_name: String,
        #[arg(long, default_value = "")]
        description: String,
        /// 自分のアカウントへ作る
        #[arg(long)]
        user_id: Option<u64>,
        /// グループ資産として作る
        #[arg(long)]
        group_id: Option<u64>,
        /// assetType を明示する（拡張子から推測できない場合）
        #[arg(long)]
        asset_type: Option<String>,
    },
    /// アセットの内容とメタデータを更新する
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
        /// 正方形の PNG / JPEG
        #[arg(long, value_name = "PATH")]
        icon: Option<std::path::PathBuf>,
        #[arg(long, value_name = "PATH")]
        preview: Option<std::path::PathBuf>,
    },
    /// アセットのメタデータを取得する
    Get {
        #[arg(long)]
        asset_id: u64,
        #[arg(long)]
        read_mask: Option<String>,
    },
    /// バージョン一覧
    Versions {
        #[arg(long)]
        asset_id: u64,
        #[arg(long, default_value_t = 20)]
        max_page_size: u32,
    },
    /// 指定バージョンへ巻き戻す
    Rollback {
        #[arg(long)]
        asset_id: u64,
        #[arg(long)]
        version: u64,
    },
    /// アーカイブする
    Archive {
        #[arg(long)]
        asset_id: u64,
    },
    /// 非同期処理の状況を確認する
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
    /// ホームページ サムネイルを差し替える
    Upload {
        #[arg(long)]
        universe_id: u64,
        #[arg(long, value_name = "PATH", required = true)]
        files: Vec<std::path::PathBuf>,
    },
    /// アップロードの処理状況
    Status {
        #[arg(long)]
        universe_id: u64,
        #[arg(long, value_delimiter = ',', required = true)]
        operation_ids: Vec<String>,
    },
    /// 登録済みサムネイルの一覧
    List {
        #[arg(long)]
        universe_id: u64,
    },
    /// サムネイルを削除する
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
    /// ゲームサーバー一覧
    List {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        place_id: u64,
        #[arg(long)]
        version: u64,
    },
    /// ゲームサーバーのログ
    Logs {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        place_id: u64,
        #[arg(long)]
        version: u64,
        #[arg(long)]
        job_id: String,
        /// 表示する件数。0 で全件。
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// 全件を出す（--limit 0 と同じ）
        #[arg(long)]
        full: bool,
    },
    /// サーバーを再起動する
    Restart {
        #[arg(long)]
        universe_id: u64,
        /// 影響予測だけ行う
        #[arg(long)]
        dry_run: bool,
    },
    /// 再起動の影響予測
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
    /// 時系列メトリクスを問い合わせる
    Metrics {
        #[arg(long)]
        universe_id: u64,
        /// 問い合わせ JSON
        #[arg(long)]
        query: String,
    },
    /// 非同期クエリの結果を取得する
    Operation {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        operation_id: String,
    },
    /// 代表的な指標の一覧
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
        /// JSON 文字列
        #[arg(long)]
        value: String,
        /// 新規作成を許可する
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
        /// Robux 整数
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
    /// クロスサーバー メッセージを配信する
    PublishMessage {
        #[arg(long)]
        universe_id: u64,
        #[arg(long)]
        topic: String,
        /// JSON 文字列
        #[arg(long)]
        message: String,
    },
}

impl Cli {
    /// 出力形式を決める。引数 > 環境変数 > 既定。
    pub fn resolve_format(&self) -> Format {
        self.format.unwrap_or_else(Format::from_env_or_json)
    }

    /// 実行する。認証が要らないコマンドはここで早期に返す。
    pub async fn run(self) -> Output {
        // カタログは認証不要。エージェントが最初に叩く。
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
            Command::Api(_) | Command::Auth(_) => unreachable!("上で処理済み"),
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

/// API カタログ。認証不要。
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
            // query 併用時はここで実装済みに絞る。
            if *implemented && query.is_some() {
                hits.retain(|e| e.implemented);
            }
            if let Some(want) = auth {
                let want = match want.as_str() {
                    "api-key" => catalog::AuthKind::ApiKey,
                    "oauth" => catalog::AuthKind::OAuth,
                    "cookie" => catalog::AuthKind::LegacyCookie,
                    other => {
                        return failure(
                            "api list",
                            crate::output::ErrorBody {
                                code: "unknown_auth",
                                message: format!("--auth に {other} は使えません"),
                                status: None,
                                detail: Some(
                                    "api-key / oauth / cookie のいずれかを指定してください".into(),
                                ),
                            },
                        );
                    }
                };
                hits.retain(|e| e.auth.contains(&want));
            }
            let impl_count = hits.iter().filter(|e| e.implemented).count();
            Output::success(
                "api list",
                format!(
                    "エンドポイント {} 件（うち実装済み {impl_count} 件）",
                    hits.len()
                ),
                Some(serde_json::to_value(&hits).unwrap_or(serde_json::Value::Null)),
            )
            .with_hints(vec![
                "`rbxoc api show <id>` で詳細を確認できます".into(),
                "`rbxoc api mutating` で破壊的操作だけを確認できます".into(),
            ])
        }
        ApiCommand::Show { id } => match catalog::find(id) {
            Some(ep) => Output::success(
                "api show",
                format!("{} {} — {}", ep.method, ep.path, ep.summary),
                serde_json::to_value(ep).ok(),
            ),
            None => {
                let similar = catalog::search(id);
                let mut out = failure(
                    "api show",
                    crate::output::ErrorBody {
                        code: "unknown_endpoint",
                        message: format!("エンドポイント {id} は未登録です"),
                        status: None,
                        detail: None,
                    },
                );
                if !similar.is_empty() {
                    out.hints.push(format!(
                        "近い ID: {}",
                        similar.iter().map(|e| e.id).collect::<Vec<_>>().join(", ")
                    ));
                }
                out.hints
                    .push("`rbxoc api list` で全一覧を確認できます".into());
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
                format!("破壊的操作 {} 件", hits.len()),
                serde_json::to_value(&hits).ok(),
            )
            .with_hints(vec![
                "これらはデータを変更します。実行前に対象 ID を確認してください".into(),
            ])
        }
    }
}

/// 認証の状態確認。疎通まで見る。
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
                    format!("認証は有効です（{} / {}）", auth.kind(), auth.masked()),
                    Some(serde_json::json!({
                        "authKind": auth.kind(),
                        "source": source,
                        "probe": format!("universe {id} を読めました"),
                    })),
                ),
                Err(err) => failure("auth check", err),
            }
        }
        None => Output::success(
            "auth check",
            format!(
                "認証情報を解決しました（{} / {}）",
                auth.kind(),
                auth.masked()
            ),
            Some(serde_json::json!({
                "authKind": auth.kind(),
                "source": source,
                "probe": null,
            })),
        )
        .with_hints(vec![
            "--universe-id を付けると実際に API を呼んで疎通まで確認できます".into(),
        ]),
    }
}
