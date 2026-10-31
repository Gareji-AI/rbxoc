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

const DIMENSIONS_COMMAND: &str = "analytics dimensions";

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

const DIMENSION_FIELDS: &[&str] = &[
    "metric",
    "dimensions",
    "startTime",
    "endTime",
    "filter",
    "granularity",
    "limit",
];

const DIMENSION_EXAMPLE: &str = "{\"metric\":\"DailyActiveUsers\",\"dimensions\":[\"Country\"],\"startTime\":\"2026-10-01T00:00:00Z\",\"endTime\":\"2026-10-08T00:00:00Z\"}";

/// Turn a provider operation path into the id `analytics operation` can poll.
///
/// Accepts a bare id, `operations/{kind}/{id}`, or the documented
/// `v1/universes/{universeId}/operations/{kind}/{id}` path. A different
/// universe or kind is rejected instead of being rebound onto the caller's
/// query.
pub fn operation_poll_id(path: &str, universe_id: u64, kind: &str) -> Result<String, String> {
    let segment = match kind {
        "metrics" => "metrics",
        "dimensions" | "dimension-values" => "dimension-values",
        other => return Err(format!("--kind does not accept {other}")),
    };
    let path = path.trim().trim_start_matches('/');
    if path.is_empty() {
        return Err(format!("operation path `{path}` is malformed"));
    }
    // A value that is not a documented operation path stays one id. `path_segment`
    // encodes `/`, `?`, `#`, spaces, and `%` so the request target cannot change shape.
    let documented = path.starts_with("v1/universes/") || path.starts_with("operations/");
    if !documented {
        return Ok(path.to_string());
    }
    if path.chars().any(|c| matches!(c, '?' | '#' | ' ' | '\\')) {
        return Err(format!("operation path `{path}` is malformed"));
    }
    let id = {
        let parts: Vec<&str> = path.split('/').collect();
        if parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
        {
            return Err(format!("operation path `{path}` is malformed"));
        }
        if parts.len() == 3 && parts[0] == "operations" {
            if parts[1] != segment {
                return Err(format!(
                    "operation path kind {} does not match requested {segment}",
                    parts[1]
                ));
            }
            parts[2]
        } else if parts.len() == 6
            && parts[0] == "v1"
            && parts[1] == "universes"
            && parts[3] == "operations"
        {
            if parts[2] != universe_id.to_string() {
                return Err(format!(
                    "operation path universe {} does not match requested {universe_id}",
                    parts[2]
                ));
            }
            if parts[4] != segment {
                return Err(format!(
                    "operation path kind {} does not match requested {segment}",
                    parts[4]
                ));
            }
            parts[5]
        } else {
            return Err(format!(
                "operation path `{path}` is not a bare id or a documented analytics operation path"
            ));
        }
    };
    if id.is_empty() || id.contains('%') || id == "." || id == ".." {
        return Err(format!("operation id `{id}` is malformed"));
    }
    Ok(id.to_string())
}

/// Parse and shape-check a query document shared by the metrics and
/// dimension-values endpoints: JSON object, known fields, required fields, a
/// non-empty `metric`.
fn validate_query_shape(
    query: &str,
    allowed: &[&str],
    required: &[&str],
    example: &str,
) -> Result<serde_json::Map<String, Value>, ErrorBody> {
    let invalid = |message: String, detail: String| ErrorBody {
        code: "invalid_query",
        message,
        status: None,
        detail: Some(detail),
        refs: None,
    };
    let body: Value = serde_json::from_str(query).map_err(|e| ErrorBody {
        code: "invalid_query",
        message: format!("--query is not valid JSON: {e}"),
        status: None,
        detail: Some(format!("Example: --query '{example}'")),
        refs: None,
    })?;
    let Some(object) = body.as_object() else {
        return Err(invalid(
            "--query must be a JSON object".into(),
            format!("Example: --query '{example}'"),
        ));
    };
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid(
            format!("--query field {key} is not accepted"),
            "Use one of the documented query fields; unknown fields are rejected.".into(),
        ));
    }
    for field in required {
        if !object.contains_key(*field) {
            return Err(invalid(
                format!("--query is missing {field}"),
                format!("Example: --query '{example}'"),
            ));
        }
    }
    if object["metric"]
        .as_str()
        .is_none_or(|metric| metric.is_empty())
    {
        return Err(invalid(
            "--query metric must be a non-empty string".into(),
            format!("Example: --query '{example}'"),
        ));
    }
    Ok(object.clone())
}

/// Reject a granularity outside the closed enum before any request is sent.
fn check_granularity(object: &serde_json::Map<String, Value>) -> Result<(), ErrorBody> {
    let Some(granularity) = object.get("granularity").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    if !GRANULARITIES.contains(&granularity) {
        return Err(ErrorBody {
            code: "invalid_query",
            message: format!("--query granularity {granularity} is not accepted"),
            status: None,
            detail: Some(format!("Use one of: {}", GRANULARITIES.join(", "))),
            refs: None,
        });
    }
    Ok(())
}

pub fn validate_metrics_query_text(query: &str) -> Result<Value, ErrorBody> {
    let object = validate_query_shape(
        query,
        QUERY_FIELDS,
        &["metric", "granularity", "startTime", "endTime"],
        QUERY_EXAMPLE,
    )?;
    check_granularity(&object)?;
    Ok(Value::Object(object))
}

pub fn validate_dimensions_query_text(query: &str) -> Result<Value, ErrorBody> {
    let object = validate_query_shape(
        query,
        DIMENSION_FIELDS,
        &["metric", "dimensions", "startTime", "endTime"],
        DIMENSION_EXAMPLE,
    )?;
    let dimensions = match object["dimensions"].as_array() {
        Some(values) if !values.is_empty() => values,
        _ => {
            return Err(ErrorBody {
                code: "invalid_query",
                message: "--query dimensions must be a non-empty array".into(),
                status: None,
                detail: Some(format!("Example: --query '{DIMENSION_EXAMPLE}'")),
                refs: None,
            });
        }
    };
    if dimensions
        .iter()
        .any(|value| value.as_str().is_none_or(str::is_empty))
    {
        return Err(ErrorBody {
            code: "invalid_query",
            message: "--query dimensions entries must be non-empty strings".into(),
            status: None,
            detail: Some(format!("Example: --query '{DIMENSION_EXAMPLE}'")),
            refs: None,
        });
    }
    check_granularity(&object)?;
    Ok(Value::Object(object))
}

/// The route, command name and catalog entry a query kind maps to.
fn query_route(kind: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match kind {
        "dimensions" => (
            "dimension-values",
            DIMENSIONS_COMMAND,
            "analytics.dimensions",
            "Accepted the dimension values query",
        ),
        _ => (
            "metrics",
            COMMAND,
            "analytics.metrics",
            "Accepted the metrics query",
        ),
    }
}

/// POST a query document and turn an asynchronous operation path into a poll
/// hint.
async fn post_query(ctx: &Ctx, universe_id: u64, kind: &str, body: &Value) -> Output {
    let (route, command, endpoint, summary) = query_route(kind);
    let path = format!("/analytics-query-api/v1/universes/{universe_id}/{route}");
    let req = ctx.client.post(ctx.url(&path)).json(body);
    // A POST that only queries time-series data. Sent at most once like every
    // non-safe method, but a transient failure is not an unknown mutation.
    match ctx.send_read_only(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure_for_endpoint(command, endpoint, err, &ctx.auth);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => {
                    return failure_for_endpoint(command, endpoint, err, &ctx.auth);
                }
            };
            let mut out = Output::success(command, summary, Some(data.clone()));
            // Asynchronous queries return an operation.
            if let Some(op) = data.get("path").and_then(|v| v.as_str()) {
                out = match operation_poll_id(op, universe_id, kind) {
                    Ok(id) => out.with_hints(vec![format!(
                        "Run `rbxoc analytics operation --universe-id {universe_id} --kind {kind} --operation-id {id}` to fetch the result"
                    )]),
                    Err(reason) => out.with_hints(vec![format!(
                        "The query was accepted, but its operation path cannot be polled: {reason}. Reconcile `{op}` before polling; no follow-up command was generated."
                    )]),
                };
            }
            out
        }
        Err(err) => failure_for_endpoint(command, endpoint, err, &ctx.auth),
    }
}

/// Query time-series metrics.
pub async fn metrics(ctx: &Ctx, universe_id: u64, query: &str) -> Output {
    let body = match validate_metrics_query_text(query) {
        Ok(body) => body,
        Err(err) => return failure(COMMAND, err),
    };
    post_query(ctx, universe_id, "metrics", &body).await
}

/// Query the values a metric's dimensions can take, for `breakdown`/`filter`.
pub async fn dimensions(ctx: &Ctx, universe_id: u64, query: &str) -> Output {
    let body = match validate_dimensions_query_text(query) {
        Ok(body) => body,
        Err(err) => return failure(DIMENSIONS_COMMAND, err),
    };
    post_query(ctx, universe_id, "dimensions", &body).await
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
    let operation_id = match operation_poll_id(operation_id, universe_id, kind) {
        Ok(id) => id,
        Err(reason) => {
            return failure(
                "analytics operation",
                ErrorBody {
                    code: "invalid_operation_path",
                    message: reason,
                    status: None,
                    detail: Some(
                        "Pass a bare operation id, or the documented path for this universe and kind. A mismatched path is not polled."
                            .into(),
                    ),
                    refs: None,
                },
            );
        }
    };
    let encoded_id = match crate::ctx::path_segment(&operation_id) {
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
