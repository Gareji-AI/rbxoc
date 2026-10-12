# rbxoc

[![CI](https://github.com/eiei114/rbxoc/actions/workflows/ci.yml/badge.svg)](https://github.com/eiei114/rbxoc/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Roblox Open Cloud を操作する Rust 製 CLI。**コーディングエージェントが使うことを前提**に設計しています。

## なぜ作ったか

既存の [`rbxcloud`](https://github.com/Sleitnick/rbxcloud)（139★）は優れた CLI ですが、最終更新が 2025-03-23 で止まっており、その後に追加された API 群に対応していません。

| 領域 | rbxcloud | rbxoc |
| --- | --- | --- |
| サムネイル差し替え | ✗ | ✅ |
| ゲームサーバーログ | ✗ | ✅ |
| Analytics クエリ | ✗ | ✅ |
| Place バージョン履歴 | ✗ | ✅ |
| Place 公開 | ✅ | ✅ |
| アセット管理 | ✅ | ✅ |
| DataStore | ✅ | ✅ |
| 課金商品 | ✗ | ✅ |

さらに、GitHub 検索の上位に並ぶ「Best 〜 2026」系リポジトリの多くは star 数だけの空リポジトリです。実体のある選択肢は限られています。

## エージェント向け設計

### 1. 決定論的発見

`rbxoc api list` が自己記述カタログを返します。エージェントは実装を探し回らずに、何ができるかを知れます。

```bash
rbxoc api list --implemented
rbxoc api list --query thumbnail
rbxoc api show place.publish
rbxoc api mutating          # 破壊的操作だけ。実行前の確認に使う
```

カタログは `src/catalog.rs` が正本で、実装と 1:1 に対応します。未実装のエントリも `implemented: false` と理由つきで載せてあるため、存在しない機能を呼ぼうとする無駄な試行が発生しません。

### 2. 機械可読出力

すべてのコマンドが同じ封筒を返します。`--format json` が既定です。

```json
{
  "ok": true,
  "command": "place publish",
  "summary": "place を公開しました（バージョン 12）",
  "data": { "universeId": 1, "placeId": 2, "version": 12 },
  "hints": ["`rbxoc place history --place-id <id>` で履歴を確認できます"]
}
```

失敗時:

```json
{
  "ok": false,
  "command": "auth",
  "summary": "認証情報が見つかりません…",
  "hints": ["RBXOC_API_KEY を設定してください", "`rbxoc auth check` で状態を確認できます"],
  "error": { "code": "auth_missing", "message": "…" }
}
```

`error.code` は安定した識別子です。HTTP エラーは `http_401` / `http_403` / `http_5xx` のように写します。エージェントは文字列ではなくコードで分岐できます。

### 3. troubleshooting guidance

失敗には必ず `hints` が付きます。HTTP ステータスごとに次の行動を返します。

- `401` → API key の再発行、`auth check` の案内
- `403` → 権限とスコープの確認、OAuth が必要な領域の指摘
- `429` → 指数バックオフと `--retry` の案内
- `5xx` → 一時障害として再試行

### 4. 秘密値の扱い

引数より環境変数とファイルを優先します。`--api-key` は履歴に残るため、エージェントには推奨しません。

探索順:

1. `--api-key`（非推奨）
2. `RBXOC_API_KEY` 環境変数
3. `--api-key-file <path>`
4. `~/.rbxoc/api-key`
5. `RBXOC_OAUTH_TOKEN` / `~/.rbxoc/oauth-token`

出力では常にマスクします（`abcd****`）。

### 5. 終了コード

| code | 意味 |
| --- | --- |
| 0 | 成功 |
| 1 | 実行時エラー（HTTP 失敗など） |
| 2 | 認証情報が見つからない |
| 64 | 引数の誤り（不明なエンドポイント ID など） |

## インストール

### mise（推奨）

```bash
mise install          # Rust / Rojo / pesde を導入
mise run build        # release バイナリをビルド
mise run install-local  # ~/.local/bin へ
```

### cargo

```bash
cargo install --path . --locked
```

## 使い方

### 認証の確認

```bash
export RBXOC_API_KEY="..."
rbxoc auth check --universe-id 123456
```

### Place 公開

```bash
rbxoc place publish --universe-id 123 --place-id 456 --file build/game.rbxlx
rbxoc place publish --universe-id 123 --place-id 456 --file build/game.rbxlx --published
rbxoc place history --place-id 456
```

### アセット

```bash
rbxoc asset create --file model.fbx --display-name "My Model" --user-id 789
rbxoc asset operation --operation-id ABC123
rbxoc asset update --asset-id 2205400862 --file v2.fbx
rbxoc asset rollback --asset-id 2205400862 --version 1
```

対応形式（20MB / リクエスト）: `.fbx` `.gltf` `.glb` `.rbxm` `.rbxmx`（Model）、`.png` `.jpeg` `.bmp` `.tga`（Image）、`.mp3` `.ogg` `.wav` `.flac`、`.mp4` `.mov`。

### サムネイル

```bash
rbxoc thumbnail upload --universe-id 123 --files a.png b.png
rbxoc thumbnail status --universe-id 123 --operation-ids ABC123
rbxoc thumbnail list --universe-id 123
```

### サーバーログ

```bash
rbxoc server list --universe-id 123 --place-id 456 --version 12
rbxoc server logs --universe-id 123 --place-id 456 --version 12 --job-id <jobId> --limit 200
```

## 認証方式の対応

Open Cloud は 3 系統の認証を持ちます。本 CLI は cookie を対象外にしています。

| 方式 | 対応 | 用途 |
| --- | --- | --- |
| `x-api-key` | ✅ | 大半のエンドポイント |
| OAuth2 Bearer | ✅ | Analytics、アラートなど |
| legacy cookie | ✗ | ブラウザセッション由来。自動化に不向き |

**注意**: 一部の領域は OAuth2 専用です。Analytics クエリは `universe.analytics:read` が必要で、API key では呼べません。`rbxoc api show <id>` の `auth` フィールドで確認できます。

## ツールチェーン

`mise.toml` が Rojo / pesde / Rust を固定します。

```bash
mise run check         # fmt / clippy / test
mise run build         # release ビルド
mise run demo          # 認証不要のカタログ出力
```

### 自動更新について

mise の `self_update.auto` と `tool_update.global_auto` は **global-only の設定**で、プロジェクトの `mise.toml` からは有効化できません。mise がセキュリティ理由で拒否します。

```text
mise WARN  self_update.auto in non-global config .../mise.toml is ignored for security reasons
```

そのため更新はプロジェクト内で完結させています。

```bash
mise run update-tools            # 最新版を確認
mise run update-tools -- --apply # 書き換えて mise install
```

`mise latest <tool>` で安定版を解決します（`ls-remote` は nightly / beta / stable の別名も返すため単独では使えません）。

## 開発

```bash
mise run check   # fmt --check / clippy -D warnings / test
```

テストは実 API を呼びません。パス組み立て、拡張子推論、応答形状の抽出、バックオフ計算、カタログ整合性を検証します。

## 未実施の確認

このリポジトリは **API key を使った実呼び出しを検証していません**。実装とコンパイル、および実 API を呼ばない単体テストのみです。各エンドポイントの実応答は未確認です。

特に以下は未確認です。

- Place 公開の実ファイル送信
- アセットアップロードの実処理
- サムネイル API（experimental のため形状が変わる可能性）
- Analytics（OAuth2 が必要）
- 実ゲームサーバーのログ取得

## ライセンス

MIT
