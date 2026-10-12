//! DataStore 操作。
//!
//! v2 の data-stores エンドポイントを使う。entry の値は JSON 文字列として
//! 保存されるため、CLI では入力 JSON をそのまま送る。

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// エントリを取得する。
pub async fn get(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    entry: &str,
    scope: Option<&str>,
) -> Output {
    let path = entry_path(universe_id, store, scope, entry);
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "datastore get",
            format!("{store}/{entry} を取得しました"),
            Some(data),
        ),
        Err(err) => failure("datastore get", err),
    }
}

/// エントリを作成・更新する。既存には ETag が要る場合がある。
pub async fn set(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    entry: &str,
    scope: Option<&str>,
    value: &str,
    allow_missing: bool,
) -> Output {
    let parsed: Value = match serde_json::from_str(value) {
        Ok(v) => v,
        Err(e) => {
            return failure(
                "datastore set",
                ErrorBody {
                    code: "invalid_value",
                    message: format!("--value が JSON として解釈できません: {e}"),
                    status: None,
                    detail: Some("例: --value '{\"coins\":100}'".into()),
                },
            );
        }
    };
    let path = entry_path(universe_id, store, scope, entry);
    // value は JSON 文字列として包む必要がある。
    let body = json!({ "value": parsed.to_string() });
    let mut req = ctx.client.patch(ctx.url(&path)).json(&body);
    if allow_missing {
        // 新規作成を許可する。既存エントリの上書き時は ETag 一致が必要。
        req = req
            .header("if-none-match", "*")
            .header("roblox-entry-attributes", "create-only");
    }
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("datastore set", err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            Output::success(
                "datastore set",
                format!("{store}/{entry} を書き込みました"),
                Some(data),
            )
        }
        Err(err) => failure("datastore set", err),
    }
}

/// エントリを削除する。
pub async fn delete(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    entry: &str,
    scope: Option<&str>,
) -> Output {
    let path = entry_path(universe_id, store, scope, entry);
    let req = ctx.client.delete(ctx.url(&path));
    match ctx.send_empty(req).await {
        Ok(_) => Output::success(
            "datastore delete",
            format!("{store}/{entry} を削除しました"),
            Some(json!({ "store": store, "entry": entry })),
        ),
        Err(err) => failure("datastore delete", err),
    }
}

/// エントリを一覧する。
pub async fn list(ctx: &Ctx, universe_id: u64, store: &str, max_page_size: u32) -> Output {
    let path = format!(
        "/cloud/v2/universes/{universe_id}/data-stores/{store}/entries?maxPageSize={max_page_size}"
    );
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("dataStoreEntries")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "datastore list",
                format!("{store} のエントリ {count} 件を取得しました"),
                Some(data),
            )
        }
        Err(err) => failure("datastore list", err),
    }
}

fn entry_path(universe_id: u64, store: &str, scope: Option<&str>, entry: &str) -> String {
    match scope {
        Some(s) => format!(
            "/cloud/v2/universes/{universe_id}/data-stores/{store}/scopes/{s}/entries/{entry}"
        ),
        None => {
            format!("/cloud/v2/universes/{universe_id}/data-stores/{store}/entries/{entry}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_without_scope() {
        assert_eq!(
            entry_path(1, "s", None, "e"),
            "/cloud/v2/universes/1/data-stores/s/entries/e"
        );
    }

    #[test]
    fn path_with_scope() {
        assert_eq!(
            entry_path(1, "s", Some("g"), "e"),
            "/cloud/v2/universes/1/data-stores/s/scopes/g/entries/e"
        );
    }
}
