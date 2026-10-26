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

use crate::ctx::{Ctx, failure, failure_for_endpoint, path_segment};
use crate::output::Output;

/// List game servers for a place version.
pub async fn list(
    ctx: &Ctx,
    universe_id: u64,
    place_id: u64,
    version_number: u64,
    page_token: Option<&str>,
) -> Output {
    let path = format!(
        "/server-management/v1/universes/{universe_id}/places/{place_id}/versions/{version_number}/game-servers"
    );
    let data = if let Some(token) = page_token.filter(|token| !token.is_empty()) {
        ctx.get_json_query(&path, &[("PageToken", token)]).await
    } else {
        ctx.get_json(&path).await
    };
    match data {
        Ok(data) => {
            let count = data
                .get("gameServers")
                .or_else(|| data.get("servers"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let job_ids = extract_job_ids(&data);
            let next_page_token = data
                .get("nextPageToken")
                .and_then(|value| value.as_str())
                .filter(|token| !token.is_empty())
                .map(str::to_string);
            let previous_page_token = data
                .get("previousPageToken")
                .and_then(|value| value.as_str())
                .filter(|token| !token.is_empty())
                .map(str::to_string);
            // The provider can return valid rows while declaring that one
            // half of the combined stream could not be fetched. Two booleans
            // keep the available rows and the missing half machine-readable.
            let active_missing = flag(&data, "activeServersFetchError");
            let shutdown_missing = flag(&data, "shutdownServersFetchError");
            let missing_sources: Vec<&str> =
                [(active_missing, "active"), (shutdown_missing, "shutdown")]
                    .into_iter()
                    .filter_map(|(missing, label)| missing.then_some(label))
                    .collect();
            let partial = !missing_sources.is_empty();
            let summary = if partial {
                format!(
                    "Fetched {count} game servers; the result is partial ({} rows unavailable)",
                    missing_sources.join(" and ")
                )
            } else {
                format!("Fetched {count} game servers")
            };
            let mut out = Output::success(
                "server list",
                summary,
                Some(json!({
                    "count": count,
                    "jobIds": job_ids,
                    "nextPageToken": next_page_token,
                    "previousPageToken": previous_page_token,
                    "totalCount": data.get("totalCount").and_then(Value::as_u64),
                    "partial": partial,
                    "missingSources": missing_sources,
                    "activeServersFetchError": active_missing,
                    "shutdownServersFetchError": shutdown_missing,
                    "response": data,
                })),
            );
            if partial {
                out = out.with_hints([format!(
                    "This list is incomplete: the provider could not fetch {} rows. The returned gameServers and page tokens are still usable",
                    missing_sources.join(" and ")
                )]);
            }
            if let Some(token) = next_page_token {
                out = out.with_hints([format!("Pass --page-token {token} to fetch the next page")]);
            }
            if let Some(first) = job_ids.first() {
                out = out.with_hints(vec![format!(
                    "Run `rbxoc server logs --universe-id {universe_id} --place-id {place_id} --version {version_number} --job-id {first}` to fetch logs"
                )]);
            }
            out
        }
        Err(err) => failure_for_endpoint("server list", "server.list", err, &ctx.auth),
    }
}

/// Read a provider fetch-error flag. Absent and `null` both mean "not declared".
fn flag(data: &Value, key: &str) -> bool {
    data.get(key).and_then(Value::as_bool).unwrap_or(false)
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
/// `--limit` is `MaxPageSize` (1–100). `0` and `--full` follow `nextPageToken`.
#[allow(clippy::too_many_arguments)]
pub async fn logs(
    ctx: &Ctx,
    universe_id: u64,
    place_id: u64,
    version_number: u64,
    job_id: &str,
    limit: usize,
    full: bool,
    page_token: Option<&str>,
) -> Output {
    let follow = full || limit == 0;
    let page_size: u32 = if follow {
        100
    } else if (1..=100).contains(&limit) {
        limit as u32
    } else {
        return failure(
            "server logs",
            crate::output::ErrorBody {
                code: "invalid_limit",
                message: "--limit must be from 1 to 100, or 0 with --full to read every page"
                    .into(),
                status: None,
                detail: Some(
                    "MaxPageSize is 1–100. --full follows nextPageToken until the provider stops."
                        .into(),
                ),
                refs: None,
            },
        );
    };
    let encoded_job = match path_segment(job_id) {
        Ok(value) => value,
        Err(err) => return failure("server logs", err),
    };
    let path = format!(
        "/server-management/v1/universes/{universe_id}/places/{place_id}/versions/{version_number}/game-servers/{encoded_job}/logs"
    );
    let mut token = page_token
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    let mut seen = Vec::new();
    let mut entries = Vec::new();
    let mut pages = 0u32;
    loop {
        pages += 1;
        if pages > 500 {
            return failure(
                "server logs",
                crate::output::ErrorBody {
                    code: "pagination_incomplete",
                    message: "stopped after 500 log pages".into(),
                    status: None,
                    detail: token
                        .as_ref()
                        .map(|token| format!("Resume with --page-token {token}")),
                    refs: None,
                },
            );
        }
        let page_size_text = page_size.to_string();
        let mut query = vec![("MaxPageSize", page_size_text.as_str())];
        if let Some(token) = token.as_deref() {
            query.push(("PageToken", token));
        }
        let data = match ctx.get_json_query(&path, &query).await {
            Ok(data) => data,
            Err(err) => {
                return failure_for_endpoint("server logs", "server.logs", err, &ctx.auth);
            }
        };
        entries.extend(extract_log_entries(&data));
        let next = data
            .get("nextPageToken")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .map(str::to_string);
        if !follow {
            let shown = entries.len();
            let mut out = Output::success(
                "server logs",
                format!("Fetched {shown} log entries"),
                Some(json!({
                    "jobId": job_id,
                    "total": shown,
                    "shown": shown,
                    "entries": entries,
                    "nextPageToken": next,
                })),
            );
            if let Some(token) = next {
                out = out.with_hints([format!(
                    "Pass --page-token {token} to fetch the next page, or --full to follow every page"
                )]);
            }
            return out;
        }
        match next {
            None => break,
            Some(next) if seen.iter().any(|seen: &String| seen == &next) => {
                return failure(
                    "server logs",
                    crate::output::ErrorBody {
                        code: "pagination_loop",
                        message: "the provider repeated a nextPageToken".into(),
                        status: None,
                        detail: Some(format!("Repeated token: {next}")),
                        refs: None,
                    },
                );
            }
            Some(next) => {
                seen.push(next.clone());
                token = Some(next);
            }
        }
    }
    let shown = entries.len();
    Output::success(
        "server logs",
        format!("Fetched {shown} log entries across {pages} page(s)"),
        Some(json!({
            "jobId": job_id,
            "total": shown,
            "shown": shown,
            "entries": entries,
            "nextPageToken": Value::Null,
        })),
    )
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
                return failure_for_endpoint("server restart", "server.restart", err, &ctx.auth);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => {
                    return failure_for_endpoint(
                        "server restart",
                        "server.restart",
                        err,
                        &ctx.auth,
                    );
                }
            };
            Output::success("server restart", "Requested a server restart", Some(data))
        }
        Err(err) => failure_for_endpoint("server restart", "server.restart", err, &ctx.auth),
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
        Err(err) => failure_for_endpoint(
            "server restart-forecast",
            "server.restart-forecast",
            err,
            &ctx.auth,
        ),
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
