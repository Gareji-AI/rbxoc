//! 分析クエリ。
//!
//! `analytics-query-api` は OAuth2 専用（`universe.analytics:read`）。API key
//! では呼べない点が他と異なる。rbxcloud 未対応の領域。
//!
//! 問い合わせは非同期の場合があり、operation を poll する形になる。

use serde_json::{Value, json};

use crate::ctx::{Ctx, failure};
use crate::output::{ErrorBody, Output};

/// 時系列メトリクスを問い合わせる。
pub async fn metrics(ctx: &Ctx, universe_id: u64, query: &str) -> Output {
    let body: Value = match serde_json::from_str(query) {
        Ok(v) => v,
        Err(e) => {
            return failure(
                "analytics metrics",
                ErrorBody {
                    code: "invalid_query",
                    message: format!("--query が JSON として解釈できません: {e}"),
                    status: None,
                    detail: Some(
                        "例: --query '{\"metrics\":[{\"metric\":\"DailyActiveUsers\"}],\"startTime\":\"2026-10-01T00:00:00Z\",\"endTime\":\"2026-10-08T00:00:00Z\",\"granularity\":\"P1D\"}'"
                            .into(),
                    ),
                },
            );
        }
    };

    let path = format!("/analytics-query-api/v1/universes/{universe_id}/metrics");
    let req = ctx.client.post(ctx.url(&path)).json(&body);
    match ctx.send(req).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err = crate::auth::error_from_response(resp).await;
                return failure("analytics metrics", err);
            }
            let data: Value = resp.json().await.unwrap_or(Value::Null);
            let mut out = Output::success(
                "analytics metrics",
                "メトリクス問い合わせを受け付けました",
                Some(data.clone()),
            );
            // 非同期の場合は operation を返す。
            if let Some(op) = data.get("path").and_then(|v| v.as_str()) {
                out = out.with_hints(vec![format!(
                    "`rbxoc analytics operation --universe-id {universe_id} --kind metrics --operation-id {}` で結果を取得できます",
                    op.strip_prefix("operations/").unwrap_or(op)
                )]);
            }
            out
        }
        Err(err) => failure("analytics metrics", err),
    }
}

/// 非同期クエリの結果を取得する。
pub async fn operation(ctx: &Ctx, universe_id: u64, kind: &str, operation_id: &str) -> Output {
    let segment = match kind {
        "metrics" => "metrics",
        "dimensions" | "dimension-values" => "dimension-values",
        other => {
            return failure(
                "analytics operation",
                ErrorBody {
                    code: "unknown_kind",
                    message: format!("--kind に {other} は使えません"),
                    status: None,
                    detail: Some("metrics か dimensions を指定してください".into()),
                },
            );
        }
    };
    let path = format!(
        "/analytics-query-api/v1/universes/{universe_id}/operations/{segment}/{operation_id}"
    );
    match ctx.get_json(&path).await {
        Ok(data) => {
            let done = data.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut out = Output::success(
                "analytics operation",
                if done {
                    "問い合わせが完了しました".to_string()
                } else {
                    "問い合わせはまだ実行中です".to_string()
                },
                Some(data),
            );
            if !done {
                out = out.with_hints(vec!["数秒待って再実行してください".into()]);
            }
            out
        }
        Err(err) => failure("analytics operation", err),
    }
}

/// メトリクスの一覧（自己記述）。エージェントが指標名を間違えないようにする。
pub async fn catalog() -> Output {
    // 公式ドキュメントの代表的な指標。網羅ではなく、よく使うものを先に出す。
    let metrics = json!([
        { "name": "DailyActiveUsers", "unit": "count", "note": "DAU" },
        { "name": "MonthlyActiveUsers", "unit": "count", "note": "MAU" },
        { "name": "AverageSessionLength", "unit": "seconds", "note": "平均セッション長" },
        { "name": "PlaytimePerUser", "unit": "seconds", "note": "1ユーザーあたりプレイ時間" },
        { "name": "Revenue", "unit": "robux", "note": "収益" },
        { "name": "PayingUsers", "unit": "count", "note": "課金ユーザー数" },
        { "name": "NewUsers", "unit": "count", "note": "新規ユーザー" },
        { "name": "RetentionD1", "unit": "ratio", "note": "翌日継続率" },
        { "name": "RetentionD7", "unit": "ratio", "note": "7日継続率" },
        { "name": "RetentionD30", "unit": "ratio", "note": "30日継続率" }
    ]);
    Output::success(
        "analytics catalog",
        "代表的な分析指標の一覧です",
        Some(json!({ "metrics": metrics })),
    )
    .with_hints(vec![
        "正確な指標名は Creator Dashboard の分析画面で確認してください".into(),
        "この API は OAuth2 が必要です（universe.analytics:read）".into(),
    ])
}
