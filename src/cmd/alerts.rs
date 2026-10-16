//! Analytics alert configuration.
//!
//! `analytics-alert-control-plane` manages alert thresholds for a resource
//! (a universe). Alerts belong with the analytics query API already covered,
//! but live on their own host path.
//!
//! The create and update request bodies are provider-defined and have grown
//! over time, so this module forwards a caller-supplied JSON document rather
//! than pinning a schema. Validation happens server side.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure, failure_for_endpoint, path_segment};
use crate::output::{ErrorBody, Output};

fn alerts_path(resource_type: &str, resource_id: &str) -> Result<String, ErrorBody> {
    let resource_type = path_segment(resource_type)?;
    let resource_id = path_segment(resource_id)?;
    Ok(format!(
        "/analytics-alert-control-plane/v1/resource/{resource_type}/id/{resource_id}/alerts"
    ))
}

fn alert_path(resource_type: &str, resource_id: &str, alert_id: &str) -> Result<String, ErrorBody> {
    Ok(format!(
        "{}/{}",
        alerts_path(resource_type, resource_id)?,
        path_segment(alert_id)?
    ))
}

/// List alert configurations for a resource.
pub async fn list(
    ctx: &Ctx,
    resource_type: &str,
    resource_id: &str,
    firing_status: Option<&str>,
) -> Output {
    let path = match alerts_path(resource_type, resource_id) {
        Ok(path) => path,
        Err(err) => return failure("analytics alerts list", err),
    };
    let query: Vec<_> = firing_status
        .map(|status| ("firingStatus", status))
        .into_iter()
        .collect();
    match ctx.get_json_query(&path, &query).await {
        Ok(data) => {
            let count = count_alerts(&data);
            Output::success(
                "analytics alerts list",
                format!("Fetched {count} alert configurations"),
                Some(data),
            )
        }
        Err(err) => failure_for_endpoint(
            "analytics alerts list",
            "analytics.alert.list",
            err,
            &ctx.auth,
        ),
    }
}

/// Count entries regardless of the envelope key the API chose.
fn count_alerts(data: &Value) -> usize {
    for key in ["alerts", "alertConfigs", "items", "data"] {
        if let Some(arr) = data.get(key).and_then(|v| v.as_array()) {
            return arr.len();
        }
    }
    data.as_array().map(|a| a.len()).unwrap_or(0)
}

/// Create an alert configuration from a JSON document.
pub async fn create(ctx: &Ctx, resource_type: &str, resource_id: &str, document: &str) -> Output {
    let body: Value = match serde_json::from_str(document) {
        Ok(v) => v,
        Err(e) => return invalid_document(e),
    };
    let path = match alerts_path(resource_type, resource_id) {
        Ok(path) => path,
        Err(err) => return failure("analytics alerts create", err),
    };
    match ctx.send(ctx.client.post(ctx.url(&path)).json(&body)).await {
        Ok(resp) => {
            finish(
                ctx,
                resp,
                "analytics alerts create",
                "analytics.alert.create",
                "Created the alert",
            )
            .await
        }
        Err(err) => failure_for_endpoint(
            "analytics alerts create",
            "analytics.alert.create",
            err,
            &ctx.auth,
        ),
    }
}

/// Update an alert configuration from a JSON document.
pub async fn update(
    ctx: &Ctx,
    resource_type: &str,
    resource_id: &str,
    alert_id: &str,
    document: &str,
) -> Output {
    let body: Value = match serde_json::from_str(document) {
        Ok(v) => v,
        Err(e) => return invalid_document(e),
    };
    let path = match alert_path(resource_type, resource_id, alert_id) {
        Ok(path) => path,
        Err(err) => return failure("analytics alerts update", err),
    };
    match ctx.send(ctx.client.patch(ctx.url(&path)).json(&body)).await {
        Ok(resp) => {
            finish(
                ctx,
                resp,
                "analytics alerts update",
                "analytics.alert.update",
                "Updated the alert",
            )
            .await
        }
        Err(err) => failure_for_endpoint(
            "analytics alerts update",
            "analytics.alert.update",
            err,
            &ctx.auth,
        ),
    }
}

/// Delete an alert configuration.
pub async fn delete(ctx: &Ctx, resource_type: &str, resource_id: &str, alert_id: &str) -> Output {
    let path = match alert_path(resource_type, resource_id, alert_id) {
        Ok(path) => path,
        Err(err) => return failure("analytics alerts delete", err),
    };
    match ctx.send_empty(ctx.client.delete(ctx.url(&path))).await {
        Ok(_) => Output::success(
            "analytics alerts delete",
            format!("Deleted alert {alert_id}"),
            Some(json!({ "alertId": alert_id })),
        )
        .with_hints(vec!["Deletes are irreversible".into()]),
        Err(err) => failure_for_endpoint(
            "analytics alerts delete",
            "analytics.alert.delete",
            err,
            &ctx.auth,
        ),
    }
}

/// List alert incidents in a time window.
pub async fn incidents(
    ctx: &Ctx,
    resource_type: &str,
    resource_id: &str,
    start_time: &str,
    end_time: &str,
) -> Output {
    let path = match alerts_path(resource_type, resource_id) {
        Ok(base) => format!("{base}/incidents"),
        Err(err) => return failure("analytics alerts incidents", err),
    };
    match ctx
        .get_json_query(&path, &[("startTime", start_time), ("endTime", end_time)])
        .await
    {
        Ok(data) => {
            let count = data
                .get("incidents")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "analytics alerts incidents",
                format!("Fetched {count} incidents"),
                Some(data),
            )
        }
        Err(err) => failure_for_endpoint(
            "analytics alerts incidents",
            "analytics.alert.incidents",
            err,
            &ctx.auth,
        ),
    }
}

fn invalid_document(e: serde_json::Error) -> Output {
    failure(
        "analytics alerts",
        ErrorBody {
            code: "invalid_document",
            message: format!("--document is not valid JSON: {e}"),
            status: None,
            detail: Some(
                "The alert schema is provider-defined; pass the exact body you want sent".into(),
            ),

            refs: None,
        },
    )
}

async fn finish(
    ctx: &Ctx,
    resp: reqwest::Response,
    command: &str,
    endpoint_id: &str,
    ok_message: &str,
) -> Output {
    if !resp.status().is_success() {
        let err = crate::auth::error_from_response(resp).await;
        return failure_for_endpoint(command, endpoint_id, err, &ctx.auth);
    }
    let data: Value = resp.json().await.unwrap_or(Value::Null);
    Output::success(command, ok_message.to_string(), Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_path_is_resource_scoped() {
        assert_eq!(
            alerts_path("universe", "123").unwrap(),
            "/analytics-alert-control-plane/v1/resource/universe/id/123/alerts"
        );
    }

    #[test]
    fn count_handles_several_envelopes() {
        assert_eq!(count_alerts(&json!({ "alerts": [1, 2, 3] })), 3);
        assert_eq!(count_alerts(&json!({ "alertConfigs": [1] })), 1);
        assert_eq!(count_alerts(&json!([1, 2])), 2);
        assert_eq!(count_alerts(&json!({})), 0);
    }
}
