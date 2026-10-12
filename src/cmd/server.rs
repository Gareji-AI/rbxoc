//! ゲームサーバーとログ。
//!
//! `server-management` は API key 専用。rbxcloud 未対応の領域。
//!
//! ログ取得の流れ:
//! 1. `server list` で place バージョンのゲームサーバー一覧を取り jobId を得る
//! 2. `server logs` に jobId を渡す
//!
//! ログ本文は巨大になり得るため、既定で件数を絞り、全文は `--full` で取る。

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::Output;

/// ゲームサーバー一覧を取得する。
pub async fn list(ctx: &Ctx, universe_id: u64, place_id: u64, version_number: u64) -> Output {
    let path = format!(
        "/server-management/v1/universes/{universe_id}/places/{place_id}/versions/{version_number}/game-servers"
    );
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("gameServers")
                .or_else(|| data.get("servers"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let job_ids = extract_job_ids(&data);
            let mut out = Output::success(
                "server list",
                format!("ゲームサーバー {count} 件を取得しました"),
                Some(json!({ "count": count, "jobIds": job_ids, "response": data })),
            );
            if let Some(first) = job_ids.first() {
                out = out.with_hints(vec![format!(
                    "`rbxoc server logs --universe-id {universe_id} --place-id {place_id} --version {version_number} --job-id {first}` でログを取得できます"
                )]);
            }
            out
        }
        Err(err) => failure("server list", err),
    }
}

/// 応答から jobId を拾う。キー名が揺れるので複数見る。
fn extract_job_ids(data: &Value) -> Vec<String> {
    let arr = ["gameServers", "servers", "items"]
        .iter()
        .find_map(|k| data.get(k).and_then(|v| v.as_array()));
    let Some(arr) = arr else { return Vec::new() };
    let mut ids = Vec::new();
    for item in arr {
        for key in ["jobId", "id", "serverId"] {
            if let Some(s) = item.get(key).and_then(|v| v.as_str()) {
                ids.push(s.to_string());
                break;
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

/// ゲームサーバーのログを取得する。
pub async fn logs(
    ctx: &Ctx,
    universe_id: u64,
    place_id: u64,
    version_number: u64,
    job_id: &str,
    limit: usize,
    full: bool,
) -> Output {
    let path = format!(
        "/server-management/v1/universes/{universe_id}/places/{place_id}/versions/{version_number}/game-servers/{job_id}/logs"
    );
    match ctx.get_json(&path).await {
        Ok(data) => {
            let entries = extract_log_entries(&data);
            let total = entries.len();
            let shown: Vec<&Value> = if full || limit == 0 {
                entries.iter().collect()
            } else {
                entries.iter().take(limit).collect()
            };
            let mut out = Output::success(
                "server logs",
                format!(
                    "ログ {total} 件を取得しました（表示 {shown_len} 件）",
                    shown_len = shown.len()
                ),
                Some(json!({
                    "jobId": job_id,
                    "total": total,
                    "shown": shown.len(),
                    "entries": shown,
                })),
            );
            if total > shown.len() {
                out = out.with_hints(vec![format!(
                    "残り {} 件は --limit 0 または --full で取得できます",
                    total - shown.len()
                )]);
            }
            out
        }
        Err(err) => failure("server logs", err),
    }
}

/// ログ項目を拾う。応答形状が一定でないため配列を探す。
fn extract_log_entries(data: &Value) -> Vec<Value> {
    for key in ["logs", "entries", "messages", "items", "data"] {
        if let Some(arr) = data.get(key).and_then(|v| v.as_array()) {
            return arr.clone();
        }
    }
    if let Some(arr) = data.as_array() {
        return arr.clone();
    }
    Vec::new()
}

/// universe のサーバー再起動を投げる。
pub async fn restart(ctx: &Ctx, universe_id: u64, dry_run: bool) -> Output {
    let path = format!("/cloud/v2/universes/{universe_id}:restartServers");
    let body = json!({ "dryRun": dry_run });
    let req = ctx.client.post(ctx.url(&path)).json(&body);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("server restart", err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            let label = if dry_run { "影響予測" } else { "再起動" };
            let mut out = Output::success(
                "server restart",
                format!("サーバー{label}を実行しました"),
                Some(data),
            );
            if dry_run {
                out = out.with_hints(vec![
                    "実際に再起動するには --dry-run を外してください".into(),
                    "影響が大きい操作です。事前に対象を確認してください".into(),
                ]);
            }
            out
        }
        Err(err) => failure("server restart", err),
    }
}

/// 再起動の影響予測。
pub async fn restart_forecast(ctx: &Ctx, universe_id: u64) -> Output {
    let path = format!("/server-management/v1/universes/{universe_id}/restarts:forecast");
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "server restart-forecast",
            "再起動の影響予測を取得しました",
            Some(data),
        ),
        Err(err) => failure("server restart-forecast", err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_job_ids_from_game_servers() {
        let data = json!({ "gameServers": [{ "jobId": "a" }, { "id": "b" }] });
        assert_eq!(extract_job_ids(&data), vec!["a", "b"]);
    }

    #[test]
    fn job_ids_are_deduped() {
        let data = json!({ "servers": [{ "jobId": "x" }, { "jobId": "x" }] });
        assert_eq!(extract_job_ids(&data), vec!["x"]);
    }

    #[test]
    fn missing_job_ids_is_empty() {
        assert!(extract_job_ids(&json!({})).is_empty());
    }

    #[test]
    fn log_entries_from_various_shapes() {
        assert_eq!(extract_log_entries(&json!({"logs": [1, 2]})).len(), 2);
        assert_eq!(extract_log_entries(&json!({"entries": [1]})).len(), 1);
        assert_eq!(extract_log_entries(&json!([1, 2, 3])).len(), 3);
        assert!(extract_log_entries(&json!({"other": 1})).is_empty());
    }
}
