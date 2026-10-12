//! API カタログ。
//!
//! エージェントが「何ができるか」を決定論的に発見するための自己記述層。
//! `rbxoc api list` / `rbxoc api show <id>` がこれを読む。
//!
//! 各エントリは Open Cloud の実エンドポイントに 1:1 で対応する。カタログを
//! 実装の正本にしているので、追加時はここを先に書く。`implemented` が false の
//! エントリは「API としては存在するが本 CLI は未対応」を表す。エージェントが
//! 未対応機能を呼ぼうとして無駄な試行をしないようにする。
//!
//! 出典: https://create.roblox.com/docs/cloud/openapi.json （2026-10-12 取得）

use serde::Serialize;

/// 認証方式。Open Cloud は 3 系統を併記する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AuthKind {
    /// `x-api-key`。自動化の第一候補。
    #[serde(rename = "api-key")]
    ApiKey,
    /// `Authorization: Bearer`。スコープが必要な領域。
    #[serde(rename = "oauth")]
    OAuth,
    /// ブラウザのセッション cookie。本 CLI は対象外。
    #[serde(rename = "legacy-cookie")]
    LegacyCookie,
}

/// 1 つのエンドポイント。
#[derive(Debug, Clone, Serialize)]
pub struct Endpoint {
    /// `resource.action` 形式の安定した ID。CLI のサブコマンド名と対応する。
    pub id: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub summary: &'static str,
    /// 本 CLI が実装済みか。未実装なら理由を `note` に書く。
    pub implemented: bool,
    pub auth: &'static [AuthKind],
    /// OAuth2 スコープ。API key 運用では不要。
    pub scopes: &'static [&'static str],
    /// 破壊的操作か。エージェントに確認を促す材料。
    pub mutating: bool,
    pub note: &'static str,
}

use AuthKind::{ApiKey, LegacyCookie, OAuth};

/// カタログ本体。実装と同時に更新する。
pub const ENDPOINTS: &[Endpoint] = &[
    // ---- 認証 ----
    Endpoint {
        id: "auth.check",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}",
        summary: "認証情報で universe を読めるか確認する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
        mutating: false,
        note: "認証の疎通確認に使う最小の読み取り。",
    },
    // ---- Place 公開 ----
    Endpoint {
        id: "place.publish",
        method: "POST",
        path: "/universes/v1/{universeId}/places/{placeId}/versions",
        summary: "RBXL / RBXLX を送って place を保存または公開する",
        implemented: true,
        auth: &[ApiKey],
        scopes: &["universe-places:write"],
        mutating: true,
        note: "versionType=Saved なら保存のみ、Published なら公開まで進む。",
    },
    Endpoint {
        id: "place.get",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}/places/{place_id}",
        summary: "place のメタデータを取得する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:read"],
        mutating: false,
        note: "",
    },
    // ---- アセット ----
    Endpoint {
        id: "asset.create",
        method: "POST",
        path: "/assets/v1/assets",
        summary: "アセットを新規アップロードする",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:write"],
        mutating: true,
        note: "1 リクエスト 20MB まで。Model は fbx / gltf / glb / rbxm / rbxmx。",
    },
    Endpoint {
        id: "asset.update",
        method: "PATCH",
        path: "/assets/v1/assets/{assetId}",
        summary: "アセットの内容とメタデータを更新する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:write"],
        mutating: true,
        note: "content body を更新できるのは Model のみ。icon と preview は Image アセットである必要がある。",
    },
    Endpoint {
        id: "asset.get",
        method: "GET",
        path: "/assets/v1/assets/{assetId}",
        summary: "アセットのメタデータを取得する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "asset.versions",
        method: "GET",
        path: "/assets/v1/assets/{assetId}/versions",
        summary: "アセットのバージョン一覧を取得する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "asset.rollback",
        method: "POST",
        path: "/assets/v1/assets/{assetId}/versions:rollback",
        summary: "アセットを指定バージョンへ巻き戻す",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "asset.archive",
        method: "POST",
        path: "/assets/v1/assets/{assetId}:archive",
        summary: "アセットをアーカイブする",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "asset.operation",
        method: "GET",
        path: "/assets/v1/operations/{operationId}",
        summary: "アップロード・更新の処理状況を確認する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["asset:read"],
        mutating: false,
        note: "create / update は非同期。operationId を poll する。",
    },
    // ---- サムネイル ----
    Endpoint {
        id: "thumbnail.upload",
        method: "POST",
        path: "/thumbnail-personalization-api/v1/universes/{universeId}/thumbnails/uploads",
        summary: "体験のホームページ サムネイルを差し替える",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.thumbnail:write"],
        mutating: true,
        note: "experimental。スキーマが予告なく変わる前提で扱う。",
    },
    Endpoint {
        id: "thumbnail.status",
        method: "GET",
        path: "/thumbnail-personalization-api/v1/universes/{universeId}/thumbnails/uploads/status",
        summary: "サムネイル アップロードの処理状況を確認する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.thumbnail:read"],
        mutating: false,
        note: "experimental。",
    },
    Endpoint {
        id: "thumbnail.list",
        method: "GET",
        path: "/thumbnail-personalization-api/v1/universes/{universeId}/thumbnails",
        summary: "登録済みホームページ サムネイルを一覧する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.thumbnail:read"],
        mutating: false,
        note: "experimental。",
    },
    Endpoint {
        id: "thumbnail.delete",
        method: "DELETE",
        path: "/thumbnail-personalization-api/v1/universes/{universeId}/thumbnails",
        summary: "ホームページ サムネイルを削除する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe.thumbnail:write"],
        mutating: true,
        note: "experimental。",
    },
    // ---- サーバー / ログ ----
    Endpoint {
        id: "server.logs",
        method: "GET",
        path: "/server-management/v1/universes/{universeId}/places/{placeId}/versions/{versionNumber}/game-servers/{jobId}/logs",
        summary: "ゲームサーバー ジョブのログを取得する",
        implemented: true,
        auth: &[ApiKey],
        scopes: &[],
        mutating: false,
        note: "rbxcloud 未対応。本 CLI の差別化領域。",
    },
    Endpoint {
        id: "server.list",
        method: "GET",
        path: "/server-management/v1/universes/{universeId}/places/{placeId}/versions/{versionNumber}/game-servers",
        summary: "place バージョンのゲームサーバー一覧を取得する",
        implemented: true,
        auth: &[ApiKey],
        scopes: &[],
        mutating: false,
        note: "jobId をここで得て server.logs へ渡す。",
    },
    Endpoint {
        id: "server.restart",
        method: "POST",
        path: "/cloud/v2/universes/{universe_id}:restartServers",
        summary: "universe のゲームサーバーを再起動する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe:write"],
        mutating: true,
        note: "forecast で影響を先に確認できる。",
    },
    // ---- 分析 ----
    Endpoint {
        id: "analytics.metrics",
        method: "POST",
        path: "/analytics-query-api/v1/universes/{universeId}/metrics",
        summary: "時系列メトリクスを問い合わせる",
        implemented: true,
        auth: &[OAuth],
        scopes: &["universe.analytics:read"],
        mutating: false,
        note: "rbxcloud 未対応。API key では不可。",
    },
    Endpoint {
        id: "analytics.dimensions",
        method: "POST",
        path: "/analytics-query-api/v1/universes/{universeId}/dimension-values",
        summary: "ディメンション値を問い合わせる",
        implemented: false,
        auth: &[OAuth],
        scopes: &["universe.analytics:read"],
        mutating: false,
        note: "未実装。metrics と同型のため後続で追加。",
    },
    // ---- Place バージョン履歴 ----
    Endpoint {
        id: "place.history",
        method: "GET",
        path: "/place-version-history-api/v1/{placeId}/history",
        summary: "place のバージョン履歴を取得する",
        implemented: true,
        auth: &[ApiKey],
        scopes: &[],
        mutating: false,
        note: "rbxcloud 未対応。",
    },
    Endpoint {
        id: "place.history.notes",
        method: "POST",
        path: "/place-version-history-api/v1/{placeId}/version/{version}/notes",
        summary: "place バージョンにノートを付ける",
        implemented: false,
        auth: &[ApiKey],
        scopes: &[],
        mutating: true,
        note: "未実装。",
    },
    // ---- データストア ----
    Endpoint {
        id: "datastore.get",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries/{entry_id}",
        summary: "DataStore エントリを取得する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe-datastores.control:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "datastore.set",
        method: "PATCH",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries/{entry_id}",
        summary: "DataStore エントリを作成・更新する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe-datastores.control:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "datastore.delete",
        method: "DELETE",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries/{entry_id}",
        summary: "DataStore エントリを削除する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe-datastores.control:write"],
        mutating: true,
        note: "",
    },
    Endpoint {
        id: "datastore.list",
        method: "GET",
        path: "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries",
        summary: "DataStore エントリを一覧する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["universe-datastores.control:read"],
        mutating: false,
        note: "",
    },
    // ---- 課金 ----
    Endpoint {
        id: "gamepass.list",
        method: "GET",
        path: "/game-passes/v1/universes/{universeId}/game-passes/creator",
        summary: "Game Pass を一覧する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["game-pass:read"],
        mutating: false,
        note: "",
    },
    Endpoint {
        id: "gamepass.create",
        method: "POST",
        path: "/game-passes/v1/universes/{universeId}/game-passes",
        summary: "Game Pass を作成する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["game-pass:write"],
        mutating: true,
        note: "multipart。imageFile を同一リクエストに載せられる。Beta。",
    },
    Endpoint {
        id: "gamepass.update",
        method: "PATCH",
        path: "/game-passes/v1/universes/{universeId}/game-passes/{gamePassId}",
        summary: "Game Pass を更新する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["game-pass:write"],
        mutating: true,
        note: "Beta。",
    },
    Endpoint {
        id: "devproduct.list",
        method: "GET",
        path: "/developer-products/v2/universes/{universeId}/developer-products/creator",
        summary: "Developer Product を一覧する",
        implemented: true,
        auth: &[ApiKey, OAuth],
        scopes: &["developer-product:read"],
        mutating: false,
        note: "Beta。",
    },
    // ---- メッセージング ----
    Endpoint {
        id: "messaging.publish",
        method: "POST",
        path: "/messaging-service/v1/universes/{universeId}/topics/{topic}",
        summary: "universe へクロスサーバー メッセージを配信する",
        implemented: true,
        auth: &[ApiKey],
        scopes: &[],
        mutating: true,
        note: "",
    },
    // ---- 未実装（存在はする） ----
    Endpoint {
        id: "media.get",
        method: "GET",
        path: "/v2/games/{universeId}/media",
        summary: "体験のメディア情報を取得する",
        implemented: false,
        auth: &[LegacyCookie],
        scopes: &[],
        mutating: false,
        note: "未実装。cookie 専用で自動化に向かないため対象外。",
    },
    Endpoint {
        id: "creatorstore.get",
        method: "GET",
        path: "/toolbox-service/v2/assets/{id}",
        summary: "Creator Store のアセットを取得する",
        implemented: false,
        auth: &[ApiKey, OAuth],
        scopes: &["creator-store-product:read"],
        mutating: false,
        note: "未実装。",
    },
];

/// ID で引く。
pub fn find(id: &str) -> Option<&'static Endpoint> {
    ENDPOINTS.iter().find(|e| e.id == id)
}

/// 部分一致で探す。エージェントの探索を助ける。
pub fn search(query: &str) -> Vec<&'static Endpoint> {
    let q = query.to_lowercase();
    ENDPOINTS
        .iter()
        .filter(|e| {
            e.id.to_lowercase().contains(&q)
                || e.summary.to_lowercase().contains(&q)
                || e.path.to_lowercase().contains(&q)
        })
        .collect()
}

/// 実装済みだけ。`api list --implemented` とテストが使う。
pub fn implemented() -> Vec<&'static Endpoint> {
    ENDPOINTS.iter().filter(|e| e.implemented).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = ENDPOINTS.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "エンドポイント ID が重複しています");
    }

    #[test]
    fn implemented_entries_have_no_placeholder_path() {
        for e in implemented() {
            assert!(!e.path.is_empty(), "{} の path が空です", e.id);
            assert!(
                e.path.starts_with('/'),
                "{} の path が / で始まりません",
                e.id
            );
        }
    }

    #[test]
    fn unimplemented_entries_explain_themselves() {
        for e in ENDPOINTS.iter().filter(|e| !e.implemented) {
            assert!(!e.note.is_empty(), "未実装 {} に理由がありません", e.id);
        }
    }

    #[test]
    fn legacy_cookie_endpoints_are_not_implemented() {
        for e in ENDPOINTS {
            if e.auth == [LegacyCookie] {
                assert!(
                    !e.implemented,
                    "{} は cookie 専用なので実装対象外です",
                    e.id
                );
            }
        }
    }

    #[test]
    fn find_and_search_work() {
        assert!(find("place.publish").is_some());
        assert!(find("nope.nope").is_none());
        let hits = search("thumbnail");
        assert!(hits.len() >= 4, "thumbnail の検索結果が少なすぎます");
    }
}
