//! Ordered DataStore operations.
//!
//! Ordered DataStores back leaderboards. Unlike standard DataStores, entries
//! carry an integer value and the API can return them sorted, which is what a
//! ranking needs.
//!
//! This is a distinct endpoint family from `/cloud/v2/.../data-stores`:
//! `/ordered-data-stores/v1/universes/{universeId}/orderedDataStores/{store}/scopes/{scope}/entries`.
//! The scope segment is required, unlike the standard DataStore v2 paths.

use serde_json::json;

use crate::ctx::{Ctx, failure, path_segment};
use crate::output::{ErrorBody, Output};

/// Read entries, newest-first by value unless `ascending` is set.
pub async fn list(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    scope: &str,
    ascending: bool,
    page_size: u32,
) -> Output {
    let path = match entries_path(universe_id, store, scope) {
        Ok(path) => path,
        Err(err) => return failure("ordered-datastore list", err),
    };
    let page_size = page_size.to_string();
    let query = [
        ("order_by", if ascending { "asc" } else { "desc" }),
        ("max_page_size", page_size.as_str()),
    ];
    match ctx.get_json_query(&path, &query).await {
        Ok(data) => {
            let count = data
                .get("entries")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let order = if ascending { "ascending" } else { "descending" };
            Output::success(
                "ordered-datastore list",
                format!("Fetched {count} entries ({order})"),
                Some(data),
            )
        }
        Err(err) => failure("ordered-datastore list", err),
    }
}

/// Read one entry.
pub async fn get(ctx: &Ctx, universe_id: u64, store: &str, scope: &str, entry: &str) -> Output {
    let path = match entry_path(universe_id, store, scope, entry) {
        Ok(path) => path,
        Err(err) => return failure("ordered-datastore get", err),
    };
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "ordered-datastore get",
            format!("Fetched {store}/{entry}"),
            Some(data),
        ),
        Err(err) => failure("ordered-datastore get", err),
    }
}

/// Create or replace an entry with an integer value.
pub async fn set(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    scope: &str,
    entry: &str,
    value: i64,
    allow_missing: bool,
) -> Output {
    let path = match entry_path(universe_id, store, scope, entry) {
        Ok(path) => path,
        Err(err) => return failure("ordered-datastore set", err),
    };
    let body = json!({ "value": value });
    let mut req = ctx.client.patch(ctx.url(&path)).json(&body);
    if allow_missing {
        req = req.query(&[("allow_missing", "true")]);
    }
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("ordered-datastore set", err);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => return failure("ordered-datastore set", err),
            };
            Output::success(
                "ordered-datastore set",
                format!("Set {store}/{entry} to {value}"),
                Some(data),
            )
        }
        Err(err) => failure("ordered-datastore set", err),
    }
}

/// Delete an entry.
pub async fn delete(ctx: &Ctx, universe_id: u64, store: &str, scope: &str, entry: &str) -> Output {
    let path = match entry_path(universe_id, store, scope, entry) {
        Ok(path) => path,
        Err(err) => return failure("ordered-datastore delete", err),
    };
    let req = ctx.client.delete(ctx.url(&path));
    match ctx.send_empty(req).await {
        Ok(_) => Output::success(
            "ordered-datastore delete",
            format!("Deleted {store}/{entry}"),
            Some(json!({ "store": store, "scope": scope, "entry": entry })),
        ),
        Err(err) => failure("ordered-datastore delete", err),
    }
}

/// Add to an entry's value atomically.
pub async fn increment(
    ctx: &Ctx,
    universe_id: u64,
    store: &str,
    scope: &str,
    entry: &str,
    amount: i64,
) -> Output {
    let path = match entry_path(universe_id, store, scope, entry) {
        Ok(path) => format!("{path}:increment"),
        Err(err) => return failure("ordered-datastore increment", err),
    };
    let body = json!({ "amount": amount });
    let req = ctx.client.post(ctx.url(&path)).json(&body);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("ordered-datastore increment", err);
            }
            let data = match ctx.json_or_error(resp).await {
                Ok(data) => data,
                Err(err) => return failure("ordered-datastore increment", err),
            };
            Output::success(
                "ordered-datastore increment",
                format!("Incremented {store}/{entry} by {amount}"),
                Some(data),
            )
        }
        Err(err) => failure("ordered-datastore increment", err),
    }
}

fn entries_path(universe_id: u64, store: &str, scope: &str) -> Result<String, ErrorBody> {
    let store = path_segment(store)?;
    let scope = path_segment(scope)?;
    Ok(format!(
        "/ordered-data-stores/v1/universes/{universe_id}/orderedDataStores/{store}/scopes/{scope}/entries"
    ))
}

fn entry_path(
    universe_id: u64,
    store: &str,
    scope: &str,
    entry: &str,
) -> Result<String, ErrorBody> {
    let entry = path_segment(entry)?;
    Ok(format!(
        "{}/{entry}",
        entries_path(universe_id, store, scope)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_path_includes_the_required_scope() {
        assert_eq!(
            entries_path(1, "board", "global").unwrap(),
            "/ordered-data-stores/v1/universes/1/orderedDataStores/board/scopes/global/entries"
        );
    }

    #[test]
    fn entry_path_appends_the_name() {
        assert!(
            entry_path(1, "board", "global", "player_42")
                .unwrap()
                .ends_with("/entries/player_42")
        );
    }

    #[test]
    fn increment_uses_the_colon_verb() {
        let path = format!("{}:increment", entry_path(1, "b", "s", "e").unwrap());
        assert!(path.ends_with("/entries/e:increment"));
    }

    #[test]
    fn named_segments_do_not_change_the_route_or_verb() {
        assert_eq!(
            entry_path(1, "a/b", "?", "#:increment").unwrap(),
            "/ordered-data-stores/v1/universes/1/orderedDataStores/a%2Fb/scopes/%3F/entries/%23%3Aincrement"
        );
        assert!(entry_path(1, "b", "..", "e").is_err());
    }
}
