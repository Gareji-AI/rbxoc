//! DataStore operations.
//!
//! Uses the v2 data-stores endpoints. Entry values are stored as JSON strings,
//! so the CLI forwards the input JSON as-is.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// Read an entry.
pub async fn get(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    entry: &str,
    scope: Option<&str>,
) -> Output {
    let path = entry_path(universe_id, store, scope, entry);
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "datastore get",
            format!("Fetched {store}/{entry}"),
            Some(data),
        ),
        Err(err) => failure("datastore get", err),
    }
}

/// Create or update an entry. Updating an existing one may need an ETag.
pub async fn set(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    entry: &str,
    scope: Option<&str>,
    value: &str,
    allow_missing: bool,
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
    let path = entry_path(universe_id, store, scope, entry);
    // The value must be wrapped as a JSON string.
    let body = json!({ "value": parsed.to_string() });
    let mut req = ctx.client.patch(ctx.url(&path)).json(&body);
    if allow_missing {
        // Permit creation. Overwriting an existing entry still needs a matching ETag.
        req = req
            .header("if-none-match", "*")
            .header("roblox-entry-attributes", "create-only");
    }
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("datastore set", err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            Output::success(
                "datastore set",
                format!("Wrote {store}/{entry}"),
                Some(data),
            )
        }
        Err(err) => failure("datastore set", err),
    }
}

/// Delete an entry.
pub async fn delete(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    entry: &str,
    scope: Option<&str>,
) -> Output {
    let path = entry_path(universe_id, store, scope, entry);
    let req = ctx.client.delete(ctx.url(&path));
    match ctx.send_empty(req).await {
        Ok(_) => Output::success(
            "datastore delete",
            format!("Deleted {store}/{entry}"),
            Some(json!({ "store": store, "entry": entry })),
        ),
        Err(err) => failure("datastore delete", err),
    }
}

/// List entries.
pub async fn list(ctx: &Ctx, universe_id: u64, store: &str, max_page_size: u32) -> Output {
    let path = format!(
        "/cloud/v2/universes/{universe_id}/data-stores/{store}/entries?maxPageSize={max_page_size}"
    );
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("dataStoreEntries")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "datastore list",
                format!("Fetched {count} entries from {store}"),
                Some(data),
            )
        }
        Err(err) => failure("datastore list", err),
    }
}

fn entry_path(universe_id: u64, store: &str, scope: Option<&str>, entry: &str) -> String {
    match scope {
        Some(s) => format!(
            "/cloud/v2/universes/{universe_id}/data-stores/{store}/scopes/{s}/entries/{entry}"
        ),
        None => {
            format!("/cloud/v2/universes/{universe_id}/data-stores/{store}/entries/{entry}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_without_scope() {
        assert_eq!(
            entry_path(1, "s", None, "e"),
            "/cloud/v2/universes/1/data-stores/s/entries/e"
        );
    }

    #[test]
    fn path_with_scope() {
        assert_eq!(
            entry_path(1, "s", Some("g"), "e"),
            "/cloud/v2/universes/1/data-stores/s/scopes/g/entries/e"
        );
    }
}
