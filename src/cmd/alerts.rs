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

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

fn alerts_path(resource_type: &str, resource_id: &str) -> String {
    format!("/analytics-alert-control-plane/v1/resource/{resource_type}/id/{resource_id}/alerts")
}

/// List alert configurations for a resource.
pub async fn list(
    ctx: &Ctx,
    resource_type: &str,
    resource_id: &str,
    firing_status: Option<&str>,
) -> Output {
    let base = alerts_path(resource_type, resource_id);
    let path = match firing_status {
        Some(status) => format!("{base}?firingStatus={status}"),
        None => base,
    };
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = count_alerts(&data);
            Output::success(
                "analytics alerts list",
                format!("Fetched {count} alert configurations"),
                Some(data),
            )
        }
        Err(err) => failure("analytics alerts list", err),
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
    let path = alerts_path(resource_type, resource_id);
    match ctx.send(ctx.client.post(ctx.url(&path)).json(&body)).await {
        Ok(resp) => finish(resp, "analytics alerts create", "Created the alert").await,
        Err(err) => failure("analytics alerts create", err),
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
    let path = format!("{}/{alert_id}", alerts_path(resource_type, resource_id));
    match ctx.send(ctx.client.patch(ctx.url(&path)).json(&body)).await {
        Ok(resp) => finish(resp, "analytics alerts update", "Updated the alert").await,
        Err(err) => failure("analytics alerts update", err),
    }
}

/// Delete an alert configuration.
pub async fn delete(ctx: &Ctx, resource_type: &str, resource_id: &str, alert_id: &str) -> Output {
    let path = format!("{}/{alert_id}", alerts_path(resource_type, resource_id));
    match ctx.send_empty(ctx.client.delete(ctx.url(&path))).await {
        Ok(_) => Output::success(
            "analytics alerts delete",
            format!("Deleted alert {alert_id}"),
            Some(json!({ "alertId": alert_id })),
        )
        .with_hints(vec!["Deletes are irreversible".into()]),
        Err(err) => failure("analytics alerts delete", err),
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
    let path = format!(
        "{}/incidents?startTime={start_time}&endTime={end_time}",
        alerts_path(resource_type, resource_id)
    );
    match ctx.get_json(&path).await {
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
        Err(err) => failure("analytics alerts incidents", err),
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
        },
    )
}

async fn finish(resp: reqwest::Response, command: &str, ok_message: &str) -> Output {
    if !resp.status().is_success() {
        let err = crate::auth::error_from_response(resp).await;
        return failure(command, err);
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
            alerts_path("universe", "123"),
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
