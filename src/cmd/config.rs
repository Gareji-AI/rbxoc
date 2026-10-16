//! Creator configs: remote configuration and feature flags.
//!
//! `creator-configs-public-api` exposes a draft/publish workflow:
//!
//! 1. `GET .../repositories/{repository}` - read the values players see
//! 2. `PATCH .../draft` - edit the draft
//! 3. `PUT .../draft:overwrite` - replace the whole draft
//! 4. `POST .../publish` - promote the draft to live
//! 5. `GET .../revisions` - list history, with restore available
//!
//! Editing a draft never affects live values, so publish is the only step that
//! changes what the game reads. The commands keep that boundary visible.

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure, path_segment};
use crate::output::{ErrorBody, Output};

fn entries_path(universe_id: u64, repository: &str) -> Result<String, ErrorBody> {
    let repository = path_segment(repository)?;
    Ok(format!(
        "/creator-configs-public-api/v1/configs/universes/{universe_id}/repositories/{repository}"
    ))
}

/// Read the live, published values.
pub async fn get(ctx: &Ctx, universe_id: u64, repository: &str) -> Output {
    let path = match entries_path(universe_id, repository) {
        Ok(base) => format!("{base}/full"),
        Err(err) => return failure("config get", err),
    };
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "config get",
            format!("Fetched published config for {repository}"),
            Some(data),
        ),
        Err(err) => failure("config get", err),
    }
}

/// Read the current draft.
pub async fn draft(ctx: &Ctx, universe_id: u64, repository: &str) -> Output {
    let path = match entries_path(universe_id, repository) {
        Ok(base) => format!("{base}/draft"),
        Err(err) => return failure("config draft", err),
    };
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "config draft",
            format!("Fetched the draft for {repository}"),
            Some(data),
        ),
        Err(err) => failure("config draft", err),
    }
}

/// Patch the draft with a JSON document.
pub async fn patch_draft(ctx: &Ctx, universe_id: u64, repository: &str, document: &str) -> Output {
    let body: Value = match serde_json::from_str(document) {
        Ok(v) => v,
        Err(e) => return invalid_document(e),
    };
    let path = match entries_path(universe_id, repository) {
        Ok(base) => format!("{base}/draft"),
        Err(err) => return failure("config patch-draft", err),
    };
    match ctx.send(ctx.client.patch(ctx.url(&path)).json(&body)).await {
        Ok(resp) => finish_write(resp, "config patch-draft", repository).await,
        Err(err) => failure("config patch-draft", err),
    }
}

/// Replace the whole draft.
pub async fn overwrite(ctx: &Ctx, universe_id: u64, repository: &str, document: &str) -> Output {
    let body: Value = match serde_json::from_str(document) {
        Ok(v) => v,
        Err(e) => return invalid_document(e),
    };
    let path = match entries_path(universe_id, repository) {
        Ok(base) => format!("{base}/draft:overwrite"),
        Err(err) => return failure("config overwrite", err),
    };
    match ctx.send(ctx.client.put(ctx.url(&path)).json(&body)).await {
        Ok(resp) => finish_write(resp, "config overwrite", repository).await,
        Err(err) => failure("config overwrite", err),
    }
}

/// Publish the draft so the game reads the new values.
pub async fn publish(ctx: &Ctx, universe_id: u64, repository: &str) -> Output {
    let path = match entries_path(universe_id, repository) {
        Ok(base) => format!("{base}/publish"),
        Err(err) => return failure("config publish", err),
    };
    match ctx
        .send(ctx.client.post(ctx.url(&path)).json(&json!({})))
        .await
    {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("config publish", err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            Output::success(
                "config publish",
                format!("Published the draft for {repository}"),
                Some(data),
            )
            .with_hints(vec![
                "The running game reads these values after it refreshes them".into(),
            ])
        }
        Err(err) => failure("config publish", err),
    }
}

/// List revisions.
pub async fn revisions(ctx: &Ctx, universe_id: u64, repository: &str) -> Output {
    let path = match entries_path(universe_id, repository) {
        Ok(base) => format!("{base}/revisions"),
        Err(err) => return failure("config revisions", err),
    };
    match ctx.get_json(&path).await {
        Ok(data) => {
            let count = data
                .get("revisions")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Output::success(
                "config revisions",
                format!("Fetched {count} revisions for {repository}"),
                Some(data),
            )
        }
        Err(err) => failure("config revisions", err),
    }
}

/// Restore a previous revision into the draft.
pub async fn restore(ctx: &Ctx, universe_id: u64, repository: &str, revision_id: &str) -> Output {
    let path = match entries_path(universe_id, repository).and_then(|base| {
        Ok(format!(
            "{base}/revisions/{}/restore",
            path_segment(revision_id)?
        ))
    }) {
        Ok(path) => path,
        Err(err) => return failure("config restore", err),
    };
    match ctx
        .send(ctx.client.post(ctx.url(&path)).json(&json!({})))
        .await
    {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("config restore", err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            Output::success(
                "config restore",
                format!("Staged a revert to revision {revision_id}"),
                Some(data),
            )
            .with_hints(vec![
                "This stages the change. Run `rbxoc config publish` to make it live".into(),
            ])
        }
        Err(err) => failure("config restore", err),
    }
}

fn invalid_document(e: serde_json::Error) -> Output {
    failure(
        "config",
        ErrorBody {
            code: "invalid_document",
            message: format!("--document is not valid JSON: {e}"),
            status: None,
            detail: Some("Example: --document '{\"flags\":{\"newShop\":true}}'".into()),

            refs: None,
        },
    )
}

/// Shared tail for draft writes.
async fn finish_write(resp: reqwest::Response, command: &str, repository: &str) -> Output {
    if !resp.status().is_success() {
        let err = crate::auth::error_from_response(resp).await;
        return failure(command, err);
    }
    let data: Value = resp.json().await.unwrap_or(Value::Null);
    Output::success(
        command,
        format!("Updated the draft for {repository}"),
        Some(data),
    )
    .with_hints(vec![
        "Draft changes are not live until `rbxoc config publish` runs".into(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_universe_and_repository_scoped() {
        let base = entries_path(1, "game").unwrap();
        assert_eq!(
            base,
            "/creator-configs-public-api/v1/configs/universes/1/repositories/game"
        );
        assert!(format!("{base}/full").ends_with("/repositories/game/full"));
        assert!(format!("{base}/draft").ends_with("/repositories/game/draft"));
        assert!(format!("{base}/draft:overwrite").ends_with("/repositories/game/draft:overwrite"));
        assert!(format!("{base}/publish").ends_with("/repositories/game/publish"));
        assert!(format!("{base}/revisions").ends_with("/repositories/game/revisions"));
    }

    #[test]
    fn restore_path_includes_the_revision() {
        let path = format!("{}/revisions/r7/restore", entries_path(1, "game").unwrap());
        assert!(path.ends_with("/repositories/game/revisions/r7/restore"));
    }

    #[test]
    fn repository_is_one_encoded_segment() {
        assert!(
            entries_path(1, "a/b?c#d %")
                .unwrap()
                .ends_with("/a%2Fb%3Fc%23d%20%25")
        );
        assert!(entries_path(1, "..").is_err());
    }
}
