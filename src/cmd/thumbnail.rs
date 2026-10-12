//! サムネイル差し替え。
//!
//! `thumbnail-personalization-api` は experimental。スキーマが予告なく変わる
//! 前提で扱う。API key か OAuth（`universe.thumbnail:write`）が必要。
//!
//! アップロードは非同期。応答の operationIds を `thumbnail status` へ渡す。

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

const COMMAND: &str = "thumbnail upload";

/// ホームページ サムネイルを差し替える。
pub async fn upload(ctx: &Ctx, universe_id: u64, files: &[std::path::PathBuf]) -> Output {
    if files.is_empty() {
        return failure(
            COMMAND,
            ErrorBody {
                code: "no_files",
                message: "アップロードする画像がありません".into(),
                status: None,
                detail: None,
            },
        );
    }

    let mut form = reqwest::multipart::Form::new();
    for file in files {
        let bytes = match tokio::fs::read(file).await {
            Ok(b) => b,
            Err(e) => {
                return failure(
                    COMMAND,
                    ErrorBody {
                        code: "file_unreadable",
                        message: format!("画像を読めません: {}", file.display()),
                        status: None,
                        detail: Some(e.to_string()),
                    },
                );
            }
        };
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("thumb.png")
            .to_string();
        let lower = name.to_ascii_lowercase();
        let ct = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            "image/jpeg"
        } else {
            "image/png"
        };
        let part = match reqwest::multipart::Part::bytes(bytes)
            .file_name(name)
            .mime_str(ct)
        {
            Ok(p) => p,
            Err(e) => {
                return failure(
                    COMMAND,
                    ErrorBody {
                        code: "mime_invalid",
                        message: format!("content-type が不正です: {e}"),
                        status: None,
                        detail: None,
                    },
                );
            }
        };
        let taken = std::mem::replace(&mut form, reqwest::multipart::Form::new());
        form = taken.part("files", part);
    }

    let path =
        format!("/thumbnail-personalization-api/v1/universes/{universe_id}/thumbnails/uploads");
    let req = ctx.client.post(ctx.url(&path)).multipart(form);

    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure(COMMAND, err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            let ops = extract_operation_ids(&data);
            let mut out = Output::success(
                COMMAND,
                format!(
                    "サムネイル {} 件を受け付けました（operation {} 件）",
                    files.len(),
                    ops.len()
                ),
                Some(json!({ "operationIds": ops, "response": data })),
            );
            if let Some(first) = ops.first() {
                out = out.with_hints(vec![
                    format!(
                        "`rbxoc thumbnail status --universe-id {universe_id} --operation-ids {first}` で処理状況を確認できます"
                    ),
                    "処理は非同期です。反映まで時間がかかります".into(),
                    "この API は experimental です。スキーマが変わる可能性があります".into(),
                ]);
            }
            out
        }
        Err(err) => failure(COMMAND, err),
    }
}

/// 応答から operation ID を拾う。形状が experimental なので複数の置き場所を見る。
fn extract_operation_ids(data: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for key in ["operationIds", "operations", "paths"] {
        if let Some(arr) = data.get(key).and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    ids.push(s.strip_prefix("operations/").unwrap_or(s).to_string());
                } else if let Some(s) = item.get("path").and_then(|v| v.as_str()) {
                    ids.push(s.strip_prefix("operations/").unwrap_or(s).to_string());
                }
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

/// アップロードの処理状況を確認する。
pub async fn status(ctx: &Ctx, universe_id: u64, operation_ids: &[String]) -> Output {
    if operation_ids.is_empty() {
        return failure(
            "thumbnail status",
            ErrorBody {
                code: "no_operation_ids",
                message: "operationIds が必要です".into(),
                status: None,
                detail: None,
            },
        );
    }
    let joined = operation_ids.join(",");
    let path = format!(
        "/thumbnail-personalization-api/v1/universes/{universe_id}/thumbnails/uploads/status?operationIds={joined}"
    );
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "thumbnail status",
            format!("operation {} 件の状況を取得しました", operation_ids.len()),
            Some(data),
        ),
        Err(err) => failure("thumbnail status", err),
    }
}

/// 登録済みサムネイルを一覧する。
pub async fn list(ctx: &Ctx, universe_id: u64) -> Output {
    let path = format!("/thumbnail-personalization-api/v1/universes/{universe_id}/thumbnails");
    match ctx.get_json(&path).await {
        Ok(data) => Output::success(
            "thumbnail list",
            format!("universe {universe_id} のサムネイルを取得しました"),
            Some(data),
        ),
        Err(err) => failure("thumbnail list", err),
    }
}

/// サムネイルを削除する。
pub async fn delete(ctx: &Ctx, universe_id: u64, thumbnail_ids: &[String]) -> Output {
    let path = format!("/thumbnail-personalization-api/v1/universes/{universe_id}/thumbnails");
    let req = ctx
        .client
        .delete(ctx.url(&path))
        .json(&json!({ "thumbnailIds": thumbnail_ids }));
    match ctx.send_empty(req).await {
        Ok(_) => Output::success(
            "thumbnail delete",
            format!("サムネイル {} 件を削除しました", thumbnail_ids.len()),
            Some(json!({ "universeId": universe_id, "removed": thumbnail_ids.len() })),
        ),
        Err(err) => failure("thumbnail delete", err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_operation_ids_from_flat_array() {
        let data = json!({ "operationIds": ["abc", "def"] });
        assert_eq!(extract_operation_ids(&data), vec!["abc", "def"]);
    }

    #[test]
    fn extracts_and_strips_prefix() {
        let data = json!({ "operations": [{ "path": "operations/xyz" }] });
        assert_eq!(extract_operation_ids(&data), vec!["xyz"]);
    }

    #[test]
    fn dedupes_and_ignores_missing() {
        let data = json!({ "operationIds": ["a", "a"], "other": 1 });
        assert_eq!(extract_operation_ids(&data), vec!["a"]);
        assert!(extract_operation_ids(&json!({})).is_empty());
    }
}
