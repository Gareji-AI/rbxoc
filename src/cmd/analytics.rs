//! Analytics queries.
//!
//! `analytics-query-api` is OAuth2-only (`universe.analytics:read`). Unlike
//! most areas, an API key cannot call it. Not covered by rbxcloud.
//!
//! Queries may be asynchronous and return an operation to poll.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure, failure_for_endpoint};
use crate::output::{ErrorBody, Output};

const COMMAND: &str = "analytics metrics";

/// Query time-series metrics.
pub async fn metrics(ctx: &Ctx, universe_id: u64, query: &str) -> Output {
    let body: Value = match serde_json::from_str(query) {
        Ok(v) => v,
        Err(e) => {
            return failure(
                COMMAND,
                ErrorBody {
                    code: "invalid_query",
                    message: format!("--query is not valid JSON: {e}"),
                    status: None,
                    detail: Some(
                        "Example: --query '{\"metrics\":[{\"metric\":\"DailyActiveUsers\"}],\"startTime\":\"2026-10-01T00:00:00Z\",\"endTime\":\"2026-10-08T00:00:00Z\",\"granularity\":\"P1D\"}'"
                            .into(),
                    ),

                    refs: None,
                },
            );
        }
    };

    let path = format!("/analytics-query-api/v1/universes/{universe_id}/metrics");
    let req = ctx.client.post(ctx.url(&path)).json(&body);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure_for_endpoint(COMMAND, "analytics.metrics", err, &ctx.auth);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            let mut out =
                Output::success(COMMAND, "Accepted the metrics query", Some(data.clone()));
            // Asynchronous queries return an operation.
            if let Some(op) = data.get("path").and_then(|v| v.as_str()) {
                out = out.with_hints(vec![format!(
                    "Run `rbxoc analytics operation --universe-id {universe_id} --kind metrics --operation-id {}` to fetch the result",
                    op.strip_prefix("operations/").unwrap_or(op)
                )]);
            }
            out
        }
        Err(err) => failure_for_endpoint(COMMAND, "analytics.metrics", err, &ctx.auth),
    }
}

/// Fetch the result of an asynchronous query.
pub async fn operation(ctx: &Ctx, universe_id: u64, kind: &str, operation_id: &str) -> Output {
    let segment = match kind {
        "metrics" => "metrics",
        "dimensions" | "dimension-values" => "dimension-values",
        other => {
            return failure(
                "analytics operation",
                ErrorBody {
                    code: "unknown_kind",
                    message: format!("--kind does not accept {other}"),
                    status: None,
                    detail: Some("Use metrics or dimensions".into()),

                    refs: None,
                },
            );
        }
    };
    let path = format!(
        "/analytics-query-api/v1/universes/{universe_id}/operations/{segment}/{operation_id}"
    );
    match ctx.get_json(&path).await {
        Ok(data) => {
            let done = data.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut out = Output::success(
                "analytics operation",
                if done {
                    "Query completed".to_string()
                } else {
                    "Query is still running".to_string()
                },
                Some(data),
            );
            if !done {
                out = out.with_hints(vec!["Wait a few seconds and run it again".into()]);
            }
            out
        }
        Err(err) => {
            failure_for_endpoint("analytics operation", "analytics.metrics", err, &ctx.auth)
        }
    }
}

/// Common metric names. Keeps agents from guessing wrong identifiers.
pub async fn catalog() -> Output {
    // Representative metrics from the official docs, not an exhaustive list.
    let metrics = json!([
        { "name": "DailyActiveUsers", "unit": "count", "note": "DAU" },
        { "name": "MonthlyActiveUsers", "unit": "count", "note": "MAU" },
        { "name": "AverageSessionLength", "unit": "seconds", "note": "average session length" },
        { "name": "PlaytimePerUser", "unit": "seconds", "note": "playtime per user" },
        { "name": "Revenue", "unit": "robux", "note": "revenue" },
        { "name": "PayingUsers", "unit": "count", "note": "paying users" },
        { "name": "NewUsers", "unit": "count", "note": "new users" },
        { "name": "RetentionD1", "unit": "ratio", "note": "day-1 retention" },
        { "name": "RetentionD7", "unit": "ratio", "note": "day-7 retention" },
        { "name": "RetentionD30", "unit": "ratio", "note": "day-30 retention" }
    ]);
    Output::success(
        "analytics catalog",
        "Common analytics metrics",
        Some(json!({ "metrics": metrics })),
    )
    .with_hints(vec![
        "Confirm exact metric names in the Creator Dashboard analytics view".into(),
        "This API requires OAuth2 (universe.analytics:read)".into(),
    ])
}
