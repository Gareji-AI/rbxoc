//! Open Cloud HTTP error parsing and permission guidance.
//!
//! Roblox error bodies usually carry `errorCode` and `message`. This module
//! turns them into stable `ErrorBody` codes and optional doc links.

use serde_json::Value;

use crate::auth;
use crate::catalog;
use crate::output::{ErrorBody, ErrorRef};

/// Parsed fields from a JSON error body.
#[derive(Debug, Default)]
struct OpenCloudErrorJson {
    error_code: Option<String>,
    message: Option<String>,
}

/// Convert an HTTP response into an `ErrorBody`.
pub async fn error_from_http(response: reqwest::Response) -> ErrorBody {
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    build_error_body(status, &body, None)
}

/// Build an `ErrorBody` from status and raw body text.
pub fn build_error_body(status: u16, body: &str, endpoint_id: Option<&str>) -> ErrorBody {
    let parsed = parse_error_json(body);
    let api_message = parsed.message.as_deref().filter(|m| !m.is_empty());
    let error_code = parsed.error_code.as_deref();

    let detail = if body.is_empty() {
        None
    } else {
        Some(body.chars().take(600).collect())
    };

    if is_feature_not_enrolled(error_code, api_message, body) {
        return ErrorBody {
            code: "feature_not_enrolled",
            message: api_message.map(str::to_string).unwrap_or_else(|| {
                "This experience is not enrolled in the required feature".into()
            }),
            status: Some(status),
            detail,
            refs: Some(vec![ErrorRef {
                label: "Open Cloud — analytics".into(),
                url: "https://create.roblox.com/docs/cloud/analytics".into(),
            }]),
        };
    }

    if is_auth_oauth_required(error_code, api_message, body, endpoint_id) {
        return ErrorBody {
            code: "auth_oauth_required",
            message: api_message.map(str::to_string).unwrap_or_else(|| {
                "This endpoint requires OAuth2 with scopes, not an API key".into()
            }),
            status: Some(status),
            detail,
            refs: oauth_required_refs(endpoint_id),
        };
    }

    let message = if let Some(msg) = api_message {
        format!("Open Cloud returned HTTP {status}: {msg}")
    } else {
        format!("Open Cloud returned HTTP {status}")
    };

    ErrorBody {
        code: auth::status_code(&status),
        message,
        status: Some(status),
        detail,
        refs: None,
    }
}

/// Adjust an error using catalog endpoint context (OAuth-only endpoints, etc.).
pub fn apply_endpoint_context(
    mut error: ErrorBody,
    endpoint_id: &str,
    auth_kind: &str,
) -> ErrorBody {
    if matches!(
        error.code,
        "feature_not_enrolled"
            | "mutation_outcome_unknown"
            | "publication_uncertain"
            | "read_outcome_unknown"
            | "unexpected_redirect"
    ) {
        return error;
    }

    let Some(endpoint) = catalog::find(endpoint_id) else {
        return error;
    };

    if matches!(error.status, Some(401 | 403))
        && auth_kind == "api-key"
        && endpoint.auth == [catalog::AuthKind::OAuth]
    {
        error.code = "auth_oauth_required";
        if error.message.starts_with("Open Cloud returned HTTP") {
            error.message = format!(
                "{} requires OAuth2 (scopes: {})",
                endpoint_id,
                endpoint.scopes.join(", ")
            );
        }
        if error.refs.is_none() {
            error.refs = oauth_required_refs(Some(endpoint_id));
        }
    }

    error
}

/// Hints for Creator Dashboard permissions when auth failed but enrollment is not the issue.
pub fn permission_hints_for_endpoint(endpoint_id: &str, status: u16) -> Vec<String> {
    if status != 401 && status != 403 {
        return Vec::new();
    }

    let Some((scopes, dashboard_group)) = catalog::permission_hints_for_endpoint_id(endpoint_id)
    else {
        return Vec::new();
    };

    let scopes_line = scopes.join(", ");
    let mut hint = format!(
        "Try enabling in Creator Dashboard → API keys → Permissions: {scopes_line} \
         (endpoint {endpoint_id}; rbxoc api show {endpoint_id})"
    );

    if let Some(group) = dashboard_group {
        hint.push_str(&format!(" — look under the \"{group}\" permission group"));
    }

    vec![hint]
}

fn nonempty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn parse_error_json(body: &str) -> OpenCloudErrorJson {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return OpenCloudErrorJson::default();
    }
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return OpenCloudErrorJson::default();
    };
    if let Value::String(message) = value {
        return OpenCloudErrorJson {
            message: nonempty(&message),
            ..OpenCloudErrorJson::default()
        };
    }
    let Some(object) = value.as_object() else {
        return OpenCloudErrorJson::default();
    };
    let field = |key: &str| object.get(key).and_then(Value::as_str).and_then(nonempty);
    let nested = object
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .and_then(nonempty);
    OpenCloudErrorJson {
        error_code: field("errorCode"),
        message: field("message").or(field("errorMessage")).or(nested),
    }
}

fn is_feature_not_enrolled(error_code: Option<&str>, message: Option<&str>, body: &str) -> bool {
    if error_code == Some("feature_not_enrolled") {
        return true;
    }
    let haystack = format!(
        "{} {} {}",
        error_code.unwrap_or(""),
        message.unwrap_or(""),
        body
    );
    haystack.contains("PerformanceMonitoringV2") || haystack.contains("feature_not_enrolled")
}

fn is_auth_oauth_required(
    error_code: Option<&str>,
    message: Option<&str>,
    body: &str,
    endpoint_id: Option<&str>,
) -> bool {
    if matches!(
        error_code,
        Some("auth_oauth_required") | Some("OAuthRequired") | Some("UnauthorizedOAuth")
    ) {
        return true;
    }

    let haystack = format!(
        "{} {} {}",
        error_code.unwrap_or(""),
        message.unwrap_or(""),
        body
    )
    .to_lowercase();

    if haystack.contains("oauth")
        || haystack.contains("bearer")
        || haystack.contains("roblox-oauth2")
        || haystack.contains("api key is not supported")
    {
        return true;
    }

    if let Some(id) = endpoint_id
        && let Some(endpoint) = catalog::find(id)
    {
        return endpoint.auth == [catalog::AuthKind::OAuth];
    }

    false
}

fn oauth_required_refs(endpoint_id: Option<&str>) -> Option<Vec<ErrorRef>> {
    let mut refs = vec![ErrorRef {
        label: "Open Cloud authentication".into(),
        url: "https://create.roblox.com/docs/cloud/auth/oauth2-overview".into(),
    }];
    if let Some(id) = endpoint_id {
        refs.push(ErrorRef {
            label: format!("Catalog entry `{id}`"),
            url: "https://create.roblox.com/docs/cloud".into(),
        });
    }
    Some(refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_error_code_and_message() {
        let body = r#"{"errorCode":"Forbidden","message":"Not allowed"}"#;
        let err = build_error_body(403, body, None);
        assert_eq!(err.code, "http_403");
        assert!(err.message.contains("Not allowed"));
    }

    #[test]
    fn classifies_feature_not_enrolled() {
        let body =
            r#"{"errorCode":"feature_not_enrolled","message":"PerformanceMonitoringV2 is off"}"#;
        let err = build_error_body(403, body, None);
        assert_eq!(err.code, "feature_not_enrolled");
        assert!(err.refs.as_ref().is_some_and(|r| !r.is_empty()));
    }

    #[test]
    fn classifies_performance_monitoring_from_message() {
        let body = r#"{"message":"Enable PerformanceMonitoringV2 first"}"#;
        let err = build_error_body(403, body, None);
        assert_eq!(err.code, "feature_not_enrolled");
    }

    #[test]
    fn classifies_oauth_required_from_api_message() {
        let body = r#"{"errorCode":"Unauthorized","message":"OAuth bearer token required"}"#;
        let err = build_error_body(401, body, None);
        assert_eq!(err.code, "auth_oauth_required");
    }

    #[test]
    fn permission_hints_include_scopes_and_show_command() {
        let hints = permission_hints_for_endpoint("analytics.alert.list", 403);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].contains("universe.analytics.alert:read"));
        assert!(hints[0].contains("analytics.alert.list"));
        assert!(hints[0].contains("rbxoc api show analytics.alert.list"));
        assert!(hints[0].contains("universe-analytics-alert"));
    }

    #[test]
    fn api_key_supported_analytics_is_not_reclassified_as_oauth_required() {
        let err = build_error_body(403, "{}", None);
        let err = apply_endpoint_context(err, "analytics.metrics", "api-key");
        assert_eq!(err.code, "http_403");
    }

    #[test]
    fn parses_documented_alternative_error_message_shapes() {
        for (body, expected) in [
            (
                r#"{"errorCode":"InvalidName","errorMessage":"Name is invalid"}"#,
                "Name is invalid",
            ),
            (
                r#"{"error":{"code":3,"message":"Query is invalid"}}"#,
                "Query is invalid",
            ),
            (
                r#""Thumbnail request is invalid""#,
                "Thumbnail request is invalid",
            ),
        ] {
            let err = build_error_body(400, body, None);
            assert_eq!(err.code, "http_400", "{body}");
            assert_eq!(err.status, Some(400));
            assert!(err.message.contains(expected), "{body}: {err:?}");
            assert_eq!(err.detail.as_deref(), Some(body));
            assert!(!err.message.is_empty());
        }
    }

    #[test]
    fn empty_and_malformed_bodies_keep_the_generic_summary() {
        for body in ["", "   ", "not-json", "{", r#"{"errorMessage":"  "}"#, "[]"] {
            let err = build_error_body(400, body, None);
            assert_eq!(
                err.message, "Open Cloud returned HTTP 400",
                "{body}: {err:?}"
            );
            assert_eq!(err.code, "http_400");
        }
    }

    #[test]
    fn top_level_message_wins_over_alternate_fields() {
        let err = build_error_body(
            400,
            r#"{"message":"primary","errorMessage":"other","error":{"message":"nested"}}"#,
            None,
        );
        assert!(err.message.contains("primary"));
        assert!(!err.message.contains("other"));
        assert!(!err.message.contains("nested"));
    }

    #[test]
    fn specialized_codes_are_not_rewritten_by_endpoint_context() {
        for code in [
            "publication_uncertain",
            "read_outcome_unknown",
            "unexpected_redirect",
            "mutation_outcome_unknown",
        ] {
            let err = apply_endpoint_context(
                ErrorBody {
                    code,
                    message: "keep".into(),
                    status: Some(403),
                    detail: None,
                    refs: None,
                },
                "analytics.dimensions",
                "api-key",
            );
            assert_eq!(err.code, code);
            assert_eq!(err.message, "keep");
        }
    }
}
