//! DataStore operations.
//!
//! Uses the v2 data-stores endpoints. Entry values retain their JSON types.
//!
//! Profile-based save systems (ProfileStore / ProfileService) keep their session
//! lock *inside* the entry value (`value.MetaData.ActiveSession`). Because an
//! update replaces the whole `value` object, writing over a locked entry resets
//! the lock while the game server keeps the stale profile in memory. `set` and
//! `delete` therefore read the entry first and fail closed when a recognised lock
//! is present, and `get` / `list` report the same lock as a derived flag.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure, failure_for_endpoint, path_segment};
use crate::output::{ErrorBody, Output};

/// A session lock recognized inside an entry value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLock {
    /// Which documented shape matched.
    pub shape: &'static str,
    pub place_id: Option<u64>,
    pub job_id: Option<String>,
    pub session_id: Option<String>,
}

impl SessionLock {
    /// The derived, agent-readable form. `active: true` means a lock matched.
    pub fn to_json(&self) -> Value {
        json!({
            "active": true,
            "shape": self.shape,
            "placeId": self.place_id,
            "jobId": self.job_id,
            "sessionId": self.session_id,
        })
    }

    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(place) = self.place_id {
            parts.push(format!("place_id {place}"));
        }
        if let Some(job) = &self.job_id {
            parts.push(format!("job_id {job}"));
        }
        if let Some(session) = &self.session_id {
            parts.push(format!("session {session}"));
        }
        if parts.is_empty() {
            self.shape.to_string()
        } else {
            parts.join(", ")
        }
    }
}

fn id_field(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
}

fn text_field(value: Option<&Value>) -> Option<String> {
    let value = value?;
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|id| id.to_string()))
        .filter(|text| !text.is_empty())
}

/// Detect a session lock in an entry value.
///
/// This is a pattern match on a JSON payload, not a schema validator. Recognized
/// shapes:
///
/// * ProfileStore / ProfileService: `value.MetaData.ActiveSession` with
///   `PlaceId`/`place_id`, `JobId`/`job_id`, `UniqueSessionId`/`unique_session_id`.
/// * A top-level `value.SessionJob` / `value.SessionJobId` string, used by
///   simpler lock conventions.
///
/// Unrecognized shapes return `None`, so non-profile saves stay writable exactly
/// as before: this does not claim knowledge of arbitrary schemas.
pub fn session_lock(value: &Value) -> Option<SessionLock> {
    let object = value.as_object()?;
    if let Some(session) = object
        .get("MetaData")
        .and_then(|meta| meta.get("ActiveSession"))
        .filter(|session| !session.is_null())
    {
        return Some(SessionLock {
            shape: "MetaData.ActiveSession",
            place_id: id_field(session.get("PlaceId")).or_else(|| id_field(session.get("placeId"))),
            job_id: text_field(session.get("JobId")).or_else(|| text_field(session.get("jobId"))),
            session_id: text_field(session.get("UniqueSessionId"))
                .or_else(|| text_field(session.get("uniqueSessionId"))),
        });
    }
    let job = text_field(object.get("SessionJob"))
        .or_else(|| text_field(object.get("SessionJobId")))
        .filter(|job| !job.is_empty() && job != "nil");
    if let Some(job) = job {
        return Some(SessionLock {
            shape: "SessionJob",
            place_id: id_field(object.get("PlaceId")).or_else(|| id_field(object.get("placeId"))),
            job_id: Some(job),
            session_id: None,
        });
    }
    None
}

/// Attach the derived `sessionLock` flag to an entry payload for read commands.
fn with_session_lock(mut data: Value) -> Value {
    let lock = data
        .get("value")
        .and_then(session_lock)
        .map(|lock| lock.to_json())
        .unwrap_or(Value::Null);
    if let Some(object) = data.as_object_mut() {
        object.insert("sessionLock".into(), lock);
    }
    data
}

/// Read an entry.
pub async fn get(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    entry: &str,
    scope: Option<&str>,
) -> Output {
    let path = match entry_path(universe_id, store, scope, entry) {
        Ok(path) => path,
        Err(err) => return failure("datastore get", err),
    };
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "datastore get",
            format!("Fetched {store}/{entry}"),
            Some(with_session_lock(data)),
        ),
        Err(err) => failure_for_endpoint("datastore get", "datastore.get", err, &ctx.auth),
    }
}

/// Caller-supplied replacement for entry metadata.
/// When every field is unset, the existing attributes and users are read back
/// and sent with the entry etag so a value write cannot clear them.
#[derive(Clone, Copy, Default)]
pub struct ValueUpdate<'a> {
    pub attributes_json: Option<&'a str>,
    pub users_json: Option<&'a str>,
    pub clear_attributes: bool,
    pub clear_users: bool,
}

/// Update an entry, creating it when missing only if `allow_missing` is set.
#[allow(clippy::too_many_arguments)]
pub async fn set(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    entry: &str,
    scope: Option<&str>,
    value: &str,
    allow_missing: bool,
    update: ValueUpdate<'_>,
    ignore_session_lock: bool,
) -> Output {
    let parsed: Value = match serde_json::from_str(value) {
        Ok(v) => v,
        Err(e) => {
            return failure(
                "datastore set",
                ErrorBody {
                    code: "invalid_value",
                    message: format!("--value is not valid JSON: {e}"),
                    status: None,
                    detail: Some("Example: --value '{\"coins\":100}'".into()),

                    refs: None,
                },
            );
        }
    };
    let explicit_attributes = match parse_metadata_field(
        update.attributes_json,
        update.clear_attributes,
        "--attributes",
        true,
    ) {
        Ok(value) => value,
        Err(err) => return failure("datastore set", err),
    };
    let explicit_users =
        match parse_metadata_field(update.users_json, update.clear_users, "--users", false) {
            Ok(value) => value,
            Err(err) => return failure("datastore set", err),
        };
    let path = match entry_path(universe_id, store, scope, entry) {
        Ok(path) => path,
        Err(err) => return failure("datastore set", err),
    };
    let current = match ctx.get_json(&path).await {
        Ok(data) => Some(data),
        Err(err) if err.status == Some(404) && allow_missing => None,
        Err(err) if err.status == Some(403) => {
            return failure_for_endpoint(
                "datastore set",
                "datastore.get",
                ErrorBody {
                    code: "datastore_read_forbidden",
                    message: "the value update reads the entry first, and that read was forbidden"
                        .into(),
                    status: Some(403),
                    detail: Some(
                        "Grant universe-datastores.objects:read as well as universe-datastores.objects:update. The PATCH was not sent."
                            .into(),
                    ),
                    refs: None,
                },
                &ctx.auth,
            );
        }
        Err(err) if err.status == Some(404) => {
            return failure(
                "datastore set",
                ErrorBody {
                    code: "entry_not_found",
                    message: format!("{store}/{entry} does not exist"),
                    status: Some(404),
                    detail: Some(
                        "Pass --create to create it, or supply the entry that should be updated. A value-only PATCH would clear attributes and users, so it was not sent."
                            .into(),
                    ),
                    refs: None,
                },
            );
        }
        Err(err) => {
            return failure_for_endpoint("datastore set", "datastore.get", err, &ctx.auth);
        }
    };
    let body = if let Some(current) = current {
        let etag = current
            .get("etag")
            .and_then(Value::as_str)
            .filter(|etag| !etag.is_empty());
        let Some(etag) = etag else {
            return failure(
                "datastore set",
                ErrorBody {
                    code: "etag_missing",
                    message: "the entry has no etag, so its attributes and users cannot be preserved safely".into(),
                    status: None,
                    detail: Some(
                        "Refusing to PATCH value alone. Partial updates clear omitted attributes and users."
                            .into(),
                    ),
                    refs: None,
                },
            );
        };
        // A profile session lock lives inside the value, so an etag match does
        // not make the write safe: replacing `value` resets the lock while the
        // game server keeps the stale profile in memory. Fail closed unless the
        // caller states the session is gone.
        if !ignore_session_lock && let Some(lock) = current.get("value").and_then(session_lock) {
            return session_locked("datastore set", store, entry, &lock);
        }
        let attributes = match &explicit_attributes {
            Some(value) => value.clone(),
            None => current
                .get("attributes")
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or_else(|| json!({})),
        };
        let users = match &explicit_users {
            Some(value) => value.clone(),
            None => current
                .get("users")
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or_else(|| json!([])),
        };
        if !attributes.is_object() || !users.is_array() {
            return failure(
                "datastore set",
                ErrorBody {
                    code: "invalid_entry_metadata",
                    message: "existing attributes must be an object and users must be an array".into(),
                    status: None,
                    detail: Some("Pass --attributes and --users explicitly, or --clear-attributes and --clear-users.".into()),
                    refs: None,
                },
            );
        }
        json!({
            "value": parsed,
            "attributes": attributes,
            "users": users,
            "etag": etag,
        })
    } else {
        let mut body = json!({ "value": parsed });
        if let Some(attributes) = explicit_attributes {
            body["attributes"] = attributes;
        }
        if let Some(users) = explicit_users {
            body["users"] = users;
        }
        body
    };
    let mut req = ctx.client.patch(ctx.url(&path)).json(&body);
    if allow_missing {
        req = req.query(&[("allowMissing", "true")]);
    }
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure_for_endpoint("datastore set", "datastore.set", err, &ctx.auth);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => {
                    return failure_for_endpoint("datastore set", "datastore.set", err, &ctx.auth);
                }
            };
            Output::success(
                "datastore set",
                format!("Wrote {store}/{entry}"),
                Some(data),
            )
        }
        Err(err) => failure_for_endpoint("datastore set", "datastore.set", err, &ctx.auth),
    }
}

/// Delete an entry.
///
/// The delete reads the entry first so the same session-lock pre-flight applies
/// and so the documented `etag` guard can be sent. Deleting a locked entry is at
/// least as destructive as overwriting it, and the previous bare DELETE gave up
/// the only concurrency guard the provider offers.
pub async fn delete(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    entry: &str,
    scope: Option<&str>,
    ignore_session_lock: bool,
) -> Output {
    let path = match entry_path(universe_id, store, scope, entry) {
        Ok(path) => path,
        Err(err) => return failure("datastore delete", err),
    };
    let current = match ctx.get_json(&path).await {
        Ok(data) => Some(data),
        Err(err) if err.status == Some(403) => {
            return failure_for_endpoint(
                "datastore delete",
                "datastore.get",
                ErrorBody {
                    code: "datastore_read_forbidden",
                    message: "the delete reads the entry first, and that read was forbidden"
                        .into(),
                    status: Some(403),
                    detail: Some(
                        "Grant universe-datastores.objects:read as well as universe-datastores.objects:delete. The DELETE was not sent."
                            .into(),
                    ),
                    refs: None,
                },
                &ctx.auth,
            );
        }
        Err(err) if err.status == Some(404) => None,
        Err(err) => {
            return failure_for_endpoint("datastore delete", "datastore.get", err, &ctx.auth);
        }
    };
    let mut req = ctx.client.delete(ctx.url(&path));
    if let Some(current) = current {
        if !ignore_session_lock && let Some(lock) = current.get("value").and_then(session_lock) {
            return session_locked("datastore delete", store, entry, &lock);
        }
        if let Some(etag) = current
            .get("etag")
            .and_then(Value::as_str)
            .filter(|etag| !etag.is_empty())
        {
            // `If-Match` is standard HTTP conditional-delete, and the provider
            // documents `etag` as sendable on delete too.
            req = req.header("if-match", etag);
        }
    }
    match ctx.send_empty(req).await {
        Ok(_) => Output::success(
            "datastore delete",
            format!("Deleted {store}/{entry}"),
            Some(json!({ "store": store, "entry": entry })),
        ),
        Err(err) => failure_for_endpoint("datastore delete", "datastore.delete", err, &ctx.auth),
    }
}

/// Fail closed on an entry held by a live game server session.
fn session_locked(command: &str, store: &str, entry: &str, lock: &SessionLock) -> Output {
    failure(
        command,
        ErrorBody {
            code: "entry_session_locked",
            message: "the entry is held by an active game server session".into(),
            status: None,
            detail: Some(format!(
                "{store}/{entry}: {}. A write would reset the lock while the server keeps writing stale data. Make the player leave first, or change the value through the game server.",
                lock.describe()
            )),
            refs: None,
        },
    )
}

fn parse_metadata_field(
    json_text: Option<&str>,
    clear: bool,
    flag: &str,
    object: bool,
) -> Result<Option<Value>, ErrorBody> {
    if json_text.is_some() && clear {
        return Err(ErrorBody {
            code: "metadata_conflict",
            message: format!("{flag} cannot be combined with its clear flag"),
            status: None,
            detail: None,
            refs: None,
        });
    }
    if clear {
        return Ok(Some(if object { json!({}) } else { json!([]) }));
    }
    let Some(json_text) = json_text else {
        return Ok(None);
    };
    let parsed = serde_json::from_str::<Value>(json_text).map_err(|err| ErrorBody {
        code: "invalid_value",
        message: format!("{flag} is not valid JSON: {err}"),
        status: None,
        detail: Some(if object {
            "Pass a JSON object, or --clear-attributes to replace attributes with {}.".into()
        } else {
            "Pass a JSON array of users/{id} paths, or --clear-users to replace users with []."
                .into()
        }),
        refs: None,
    })?;
    let valid = if object {
        parsed.is_object()
    } else {
        parsed.is_array()
    };
    if !valid {
        return Err(ErrorBody {
            code: "invalid_value",
            message: format!(
                "{flag} must be a JSON {}",
                if object { "object" } else { "array" }
            ),
            status: None,
            detail: None,
            refs: None,
        });
    }
    Ok(Some(parsed))
}

/// List entries.
pub async fn list(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    max_page_size: u32,
    page_token: Option<&str>,
) -> Output {
    let encoded_store = match path_segment(store) {
        Ok(value) => value,
        Err(err) => return failure("datastore list", err),
    };
    let path = format!("/cloud/v2/universes/{universe_id}/data-stores/{encoded_store}/entries");
    let page_size = max_page_size.to_string();
    let mut query = vec![("maxPageSize", page_size.as_str())];
    if let Some(token) = page_token.filter(|token| !token.is_empty()) {
        query.push(("pageToken", token));
    }
    match ctx.get_json_query(&path, &query).await {
        Ok(data) => {
            let count = data
                .get("dataStoreEntries")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            // List responses populate only path/id; the flag stays null unless a
            // value happens to be present, so it never claims knowledge it lacks.
            let mut data = data;
            if let Some(entries) = data
                .get_mut("dataStoreEntries")
                .and_then(Value::as_array_mut)
            {
                for entry in entries.iter_mut() {
                    let lock = entry
                        .get("value")
                        .and_then(session_lock)
                        .map(|lock| lock.to_json())
                        .unwrap_or(Value::Null);
                    if let Some(object) = entry.as_object_mut() {
                        object.insert("sessionLock".into(), lock);
                    }
                }
            }
            let mut out = Output::success(
                "datastore list",
                format!("Fetched {count} entries from {store}"),
                Some(data.clone()),
            );
            if let Some(token) = data.get("nextPageToken").and_then(|value| value.as_str())
                && !token.is_empty()
            {
                out = out.with_hints([format!("Pass --page-token {token} to fetch the next page")]);
            }
            out
        }
        Err(err) => failure_for_endpoint("datastore list", "datastore.list", err, &ctx.auth),
    }
}

fn entry_path(
    universe_id: u64,
    store: &str,
    scope: Option<&str>,
    entry: &str,
) -> Result<String, ErrorBody> {
    let store = path_segment(store)?;
    let entry = path_segment(entry)?;
    Ok(match scope {
        Some(s) => {
            let s = path_segment(s)?;
            format!(
                "/cloud/v2/universes/{universe_id}/data-stores/{store}/scopes/{s}/entries/{entry}"
            )
        }
        None => {
            format!("/cloud/v2/universes/{universe_id}/data-stores/{store}/entries/{entry}")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_without_scope() {
        assert_eq!(
            entry_path(1, "s", None, "e").unwrap(),
            "/cloud/v2/universes/1/data-stores/s/entries/e"
        );
    }

    #[test]
    fn path_with_scope() {
        assert_eq!(
            entry_path(1, "s", Some("g"), "e").unwrap(),
            "/cloud/v2/universes/1/data-stores/s/scopes/g/entries/e"
        );
    }

    #[test]
    fn all_named_segments_are_encoded() {
        assert_eq!(
            entry_path(1, "a/b", Some("?"), "# %日本").unwrap(),
            "/cloud/v2/universes/1/data-stores/a%2Fb/scopes/%3F/entries/%23%20%25%E6%97%A5%E6%9C%AC"
        );
        assert!(entry_path(1, "..", None, "e").is_err());
    }
}
