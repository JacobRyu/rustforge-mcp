# 実装計画（Phased Implementation Plan）

未実装部分を 5 フェーズで順次実装する計画です。
各フェーズは独立して動作確認（`cargo check` / `cargo clippy` / `cargo test`）を行います。

> **ステータス: ✅ 全 5 フェーズ実装完了（2026-09-02）**
> 全フェーズの実装と検証（fmt / clippy / test 19 件）が完了しました。
> 実装後のコードは [IMPLEMENTATION.md](./IMPLEMENTATION.md) §2 各モジュールと対応しています。

## フェーズ構成

### Phase 1: Providers 実通信
- `providers/` に LLM / MCP クライアントを実装
  - **LLM**: OpenAI 互換 `/v1/chat/completions` 相当を `reqwest` で呼び出し
  - **MCP**: rmcp `StreamableHttpClientTransport`（streamable HTTP）で接続し `call_tool` を実行
- `POST /route` を拡張: `message`/`tool` を含む場合、選択したエンドポイントへ実際にフォワードして結果を返す
- 依存: `rmcp` に `client`, `transport-streamable-http-client-reqwest`

### Phase 2: audit_logs 接続 + 監査 API
- `Storage` トレイトに `record_audit_log` / `list_audit_logs` を追加（InMemory / PostgreSQL 両実装）
- `AuditLog` モデル追加
- `POST /servers` / `DELETE /servers/:id` / `POST /route` で監査ログ記録
- `GET /audit-logs` API 追加

### Phase 3: ルーティング戦略拡張
- `RoutingStrategy`: `Score`（既存）/ `WeightedRandom` / `RoundRobin`
- リクエスト経由で戦略指定、未指定時は `ROUTING_STRATEGY` env の既定値
- サーキットブレーカー: `metadata.consecutive_failures` を監視し閾値超で Unhealthy 扱い
- 依存: `rand`

### Phase 4: コマンド安全性
- `WORKSPACE_ROOT`（作業ディレクトリ制限）、`CARGO_ALLOWED_COMMANDS`（許可コマンド）、`CARGO_TIMEOUT_SECS`（タイムアウト）
- cargo / ファイルツールで安全検証を適用

### Phase 5: セキュリティ強化
- `ENCRYPTION_KEY`（AES-256-GCM）による `api_key` の保存時暗号化
- `VIEWER_API_TOKEN`（読み取り専用 token）による軽量 RBAC
- 依存: `aes-gcm`, `base64`

## 検証
- 各フェーズ: `cargo check`
- 全フェーズ後: `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test`
- 単体テスト: router 戦略、storage、暗号化、LLM client（axum モック）、cargo 検証ロジック

## リスクと緩和
- **rmcp の client feature 追加によるビルド競合** → ビルド検証を最初に行う
- **MCP 実サーバ不要のテスト性** → ネットワーク不要のテスト設計で対応
- **既存 API 互換性** → `POST /route` の既存フィールド `kind` は必須のまま後方互換