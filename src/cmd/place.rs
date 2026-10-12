//! Place 公開・取得。
//!
//! `POST /universes/v1/{universeId}/places/{placeId}/versions` は RBXL /
//! RBXLX を `data-binary` として送る。`versionType` が `Saved` なら保存のみ、
//! `Published` なら公開まで進む。
//!
//! 応答は place のバージョン番号を返す。壊れたファイルは HTTP 400 になる。

use serde_json::json;

use crate::ctx::{Ctx, failure};
use crate::output::Output;

const COMMAND: &str = "place publish";

/// place を保存または公開する。
pub async fn publish(
    ctx: &Ctx,
    universe_id: u64,
    place_id: u64,
    file: &std::path::Path,
    published: bool,
) -> Output {
    let bytes = match tokio::fs::read(file).await {
        Ok(b) => b,
        Err(e) => {
            return failure(
                COMMAND,
                crate::output::ErrorBody {
                    code: "file_unreadable",
                    message: format!("ファイルを読めません: {}", file.display()),
                    status: None,
                    detail: Some(e.to_string()),
                },
            );
        }
    };

    let version_type = if published { "Published" } else { "Saved" };
    let path = format!(
        "/universes/v1/{universe_id}/places/{place_id}/versions?versionType={version_type}"
    );
    let req = ctx.client.post(ctx.url(&path)).body(bytes);

    match ctx.send(req).await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure(COMMAND, err);
            }
            let body = resp.text().await.unwrap_or_default();
            // 応答はバージョン番号そのもの（JSON ではない）。
            let version: Option<u64> = body.trim().parse().ok();
            let action = if published { "公開" } else { "保存" };
            let summary = match version {
                Some(v) => format!("place を{action}しました（バージョン {v}）"),
                None => format!("place を{action}しました"),
            };
            let mut hints =
                vec!["`rbxoc place history --place-id <id>` で履歴を確認できます".into()];
            if !published {
                hints.push("公開まで進めるには --published を付けてください".into());
            }
            Output::success(
                COMMAND,
                summary,
                Some(json!({
                    "universeId": universe_id,
                    "placeId": place_id,
                    "version": version,
                    "versionType": version_type,
                    "bytes": status,
                    "action": action,
                })),
            )
            .with_hints(hints)
        }
        Err(err) => failure(COMMAND, err),
    }
}

/// place のメタデータを取得する。
pub async fn get(ctx: &Ctx, universe_id: u64, place_id: u64) -> Output {
    let path = format!("/cloud/v2/universes/{universe_id}/places/{place_id}");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let name = data
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("(名称なし)");
            Output::success(
                "place get",
                format!("place を取得しました: {name}"),
                Some(data),
            )
        }
        Err(err) => failure("place get", err),
    }
}

/// place のバージョン履歴を取得する。
pub async fn history(ctx: &Ctx, place_id: u64, limit: u32) -> Output {
    let path = format!("/place-version-history-api/v1/{place_id}/history?limit={limit}");
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("versions")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "place history",
                format!("place {place_id} のバージョン {count} 件を取得しました"),
                Some(data),
            )
        }
        Err(err) => failure("place history", err),
    }
}
