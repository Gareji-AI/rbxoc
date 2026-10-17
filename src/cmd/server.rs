//! Server and log operations.
//!
//! `server-management` supports API keys and OAuth2. Not covered by rbxcloud.
//!
//! Log retrieval flow:
//! 1. `server list` gives the game servers for a place version, including jobIds
//! 2. pass a jobId to `server logs`
//!
//! Log bodies can be large, so the default caps the count. Use `--full` for all.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure, path_segment};
use crate::output::Output;

/// List game servers for a place version.
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
                format!("Fetched {count} game servers"),
                Some(json!({ "count": count, "jobIds": job_ids, "response": data })),
            );
            if let Some(first) = job_ids.first() {
                out = out.with_hints(vec![format!(
                    "Run `rbxoc server logs --universe-id {universe_id} --place-id {place_id} --version {version_number} --job-id {first}` to fetch logs"
                )]);
            }
            out
        }
        Err(err) => failure("server list", err),
    }
}

/// Pull jobIds out of the response. Key names vary, so check several.
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

/// Fetch logs for a game server.
#[allow(clippy::too_many_arguments)]
pub async fn logs(
    ctx: &Ctx,
    universe_id: u64,
    place_id: u64,
    version_number: u64,
    job_id: &str,
    limit: usize,
    full: bool,
) -> Output {
    let encoded_job = match path_segment(job_id) {
        Ok(value) => value,
        Err(err) => return failure("server logs", err),
    };
    let path = format!(
        "/server-management/v1/universes/{universe_id}/places/{place_id}/versions/{version_number}/game-servers/{encoded_job}/logs"
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
                format!("Fetched {total} log entries (showing {})", shown.len()),
                Some(json!({
                    "jobId": job_id,
                    "total": total,
                    "shown": shown.len(),
                    "entries": shown,
                })),
            );
            if total > shown.len() {
                out = out.with_hints(vec![format!(
                    "The remaining {} entries need --limit 0 or --full",
                    total - shown.len()
                )]);
            }
            out
        }
        Err(err) => failure("server logs", err),
    }
}

/// Pull log entries out. The response shape is not fixed, so find the array.
fn extract_log_entries(data: &Value) -> Vec<Value> {
    for key in [
        "gameServerLogs",
        "logs",
        "entries",
        "messages",
        "items",
        "data",
    ] {
        if let Some(arr) = data.get(key).and_then(|v| v.as_array()) {
            return arr.clone();
        }
    }
    if let Some(arr) = data.as_array() {
        return arr.clone();
    }
    Vec::new()
}

/// Trigger a universe server restart.
pub async fn restart(ctx: &Ctx, universe_id: u64, dry_run: bool) -> Output {
    if dry_run {
        let mut out = restart_forecast(ctx, universe_id).await;
        out.command = "server restart".into();
        return out.with_hints(vec![
            "Drop --dry-run to request a real restart after reviewing the forecast".into(),
            "This is a high-impact operation. Confirm the target first".into(),
        ]);
    }
    let path = format!("/cloud/v2/universes/{universe_id}:restartServers");
    let req = ctx.client.post(ctx.url(&path)).json(&json!({}));
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("server restart", err);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => return failure("server restart", err),
            };
            Output::success("server restart", "Requested a server restart", Some(data))
        }
        Err(err) => failure("server restart", err),
    }
}

/// Forecast the impact of a restart.
pub async fn restart_forecast(ctx: &Ctx, universe_id: u64) -> Output {
    let path = format!("/server-management/v1/universes/{universe_id}/restarts:forecast");
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "server restart-forecast",
            "Fetched the restart impact forecast",
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
