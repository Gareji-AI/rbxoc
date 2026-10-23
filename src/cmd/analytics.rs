//! Analytics queries.
//!
//! `analytics-query-api` supports API keys and OAuth2
//! (`universe.analytics:read`). Not covered by rbxcloud.
//!
//! Queries may be asynchronous and return an operation to poll.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure, failure_for_endpoint};
use crate::output::{ErrorBody, Output};

const COMMAND: &str = "analytics metrics";

pub const GRANULARITIES: &[&str] = &[
    "OneMinute",
    "HalfHour",
    "OneHour",
    "OneDay",
    "OneWeek",
    "OneMonth",
    "None",
];

const QUERY_FIELDS: &[&str] = &[
    "metric",
    "granularity",
    "startTime",
    "endTime",
    "breakdown",
    "filter",
    "limit",
];

const QUERY_EXAMPLE: &str = "{\"metric\":\"DailyActiveUsers\",\"granularity\":\"OneDay\",\"startTime\":\"2026-10-01T00:00:00Z\",\"endTime\":\"2026-10-08T00:00:00Z\"}";

pub fn validate_metrics_query_text(query: &str) -> Result<Value, ErrorBody> {
    let body: Value = serde_json::from_str(query).map_err(|e| ErrorBody {
        code: "invalid_query",
        message: format!("--query is not valid JSON: {e}"),
        status: None,
        detail: Some(format!("Example: --query '{QUERY_EXAMPLE}'")),
        refs: None,
    })?;
    let Some(object) = body.as_object() else {
        return Err(ErrorBody {
            code: "invalid_query",
            message: "--query must be a JSON object".into(),
            status: None,
            detail: Some(format!("Example: --query '{QUERY_EXAMPLE}'")),
            refs: None,
        });
    };
    if let Some(key) = object
        .keys()
        .find(|key| !QUERY_FIELDS.contains(&key.as_str()))
    {
        return Err(ErrorBody {
            code: "invalid_query",
            message: format!("--query field {key} is not in QueryRequest"),
            status: None,
            detail: Some(
                "Use singular metric and a granularity such as OneDay. metrics and P1D are not accepted."
                    .into(),
            ),
            refs: None,
        });
    }
    for required in ["metric", "granularity", "startTime", "endTime"] {
        if !object.contains_key(required) {
            return Err(ErrorBody {
                code: "invalid_query",
                message: format!("--query is missing {required}"),
                status: None,
                detail: Some(format!("Example: --query '{QUERY_EXAMPLE}'")),
                refs: None,
            });
        }
    }
    if object["metric"]
        .as_str()
        .is_none_or(|metric| metric.is_empty())
    {
        return Err(ErrorBody {
            code: "invalid_query",
            message: "--query metric must be a non-empty string".into(),
            status: None,
            detail: Some(format!("Example: --query '{QUERY_EXAMPLE}'")),
            refs: None,
        });
    }
    let granularity = object["granularity"].as_str().unwrap_or("");
    if !GRANULARITIES.contains(&granularity) {
        return Err(ErrorBody {
            code: "invalid_query",
            message: format!("--query granularity {granularity} is not accepted"),
            status: None,
            detail: Some(format!("Use one of: {}", GRANULARITIES.join(", "))),
            refs: None,
        });
    }
    Ok(body)
}

/// Query time-series metrics.
pub async fn metrics(ctx: &Ctx, universe_id: u64, query: &str) -> Output {
    let body = match validate_metrics_query_text(query) {
        Ok(body) => body,
        Err(err) => return failure(COMMAND, err),
    };

    let path = format!("/analytics-query-api/v1/universes/{universe_id}/metrics");
    let req = ctx.client.post(ctx.url(&path)).json(&body);
    // A POST that only queries time-series data. Sent at most once like every
    // non-safe method, but a transient failure is not an unknown mutation.
    match ctx.send_read_only(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure_for_endpoint(COMMAND, "analytics.metrics", err, &ctx.auth);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => {
                    return failure_for_endpoint(COMMAND, "analytics.metrics", err, &ctx.auth);
                }
            };
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
    let encoded_id = match crate::ctx::path_segment(operation_id) {
        Ok(value) => value,
        Err(err) => return failure("analytics operation", err),
    };
    let path = format!(
        "/analytics-query-api/v1/universes/{universe_id}/operations/{segment}/{encoded_id}"
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
            failure_for_endpoint("analytics operation", "analytics.operation", err, &ctx.auth)
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
        "API keys and OAuth2 are supported; enable universe.analytics:read (see `rbxoc api show analytics.metrics`)".into(),
    ])
}
