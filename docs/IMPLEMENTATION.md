# 現状の実装内容（Current Implementation）

このドキュメントは、リポジトリの現在の状態（`develop` ブランチ）における実装内容を、モジュール単位で整理したものです。
設計意図は [ARCHITECTURE.md](./ARCHITECTURE.md)、将来像は [ROADMAP.md](./ROADMAP.md)、実装計画は [PLAN.md](./PLAN.md) を参照してください。

概観: **全 5 フェーズの未実装部分が実装され、プロジェクトは一通り機能する状態（MCP ツール層 + ルーティング基盤 + プロバイダ実通信 + 監査 + セキュリティ）** になっています。
検証は `cargo fmt --check` / `cargo clippy -D warnings` / `cargo test`（19 件）で実施済みです。

---

## 1. モジュール構成と役割

```
src/
├── main.rs        # エントリポイント（起動・DI・タスク起動）
├── api/           # Management API（HTTP, axum / 監査・ルーティング・RBAC）
├── auth/          # 認証ミドルウェア（Admin / Viewer）とユーザ識別
├── config/        # 環境変数による設定（安全性設定含む）
├── models/        # ドメインモデル（Enum / 構造体）
├── providers/     # 外部 MCP / LLM プロバイダ実通信クライアント
├── router/        # ルーティングエンジン（3 戦略 + サーキットブレーカー）
├── security/      # AES-256-GCM による秘密情報の保存時暗号化
├── storage/       # 永続化層（PostgreSQL / InMemory、暗号化対応）
├── telemetry/     # ヘルスチェック Worker / Prometheus メトリクス
└── tools/         # MCP ツール定義（RustForgeServer）+ コマンド安全化
```

---

## 2. 各モジュールの実装内容

### 2.1 `main.rs` — 起動と依存注入
- `AppConfig::from_env()` で設定を読み込み（ルーティング戦略・安全性設定・暗号キー含む）。
- `Storage` をトレイトオブジェクト（`Arc<dyn Storage>`）として DI。
  - `USE_INMEMORY_STORAGE=true` なら `InMemoryStorage`、それ以外なら `PostgresStorage`（`DATABASE_URL` 必須）。
  - PostgreSQL 使用時、`ENCRYPTION_KEY` で AES-256-GCM の暗号オブジェクトを生成して注入。未設定なら警告ログを出して平文保存。
- 3 系統を並列で起動:
  1. **Management API**（axum を `tokio::spawn` でバックグラウンド起動、`BIND_ADDR` に bind）
  2. **Health Check Worker**（`telemetry::health_check_worker`）
  3. **MCP Server**（stdio トランスポートで `RustForgeServer::new(config.safety).serve(transport)`）
- `AppState` に storage / router / token / 既定戦略を共有して各層へ配布。

### 2.2 `config/mod.rs` — 設定
環境変数から設定を組み立てるシンプルな構造体（`env` 直読み）。
| 変数 | 既定値 | 略 |
| --- | --- | --- |
| `MANAGEMENT_API_TOKEN` | - | 管理 API トークン（必須・Admin 権限） |
| `VIEWER_API_TOKEN` | - | 読み取り専用トークン（Viewer 権限） |
| `DATABASE_URL` | - | PostgreSQL 接続文字列（条件付き必須） |
| `BIND_ADDR` | `0.0.0.0:3000` | HTTP API bind アドレス |
| `USE_INMEMORY_STORAGE` | `false` | インメモリ保存に切替 |
| `ROUTING_STRATEGY` | `score` | 既定ルーティング戦略 |
| `ENCRYPTION_KEY` | - | AES-256-GCM キー（32 バイト = 64 hex 文字） |
| `WORKSPACE_ROOT` | `.` | MCP ツールが扱える作業ルート |

`SafetyConfig`（`CARGO_ALLOWED_COMMANDS` / `CARGO_TIMEOUT_SECS` / `WORKSPACE_ROOT`）にコマンド実行・ファイル操作の安全設定を集約。

### 2.3 `models/mod.rs` — ドメインモデル
- `ProviderKind`: `Mcp` / `Llm`
- `HealthState`: `Healthy` / `Unhealthy` / `Degraded` / `Unknown`
- `ServerEndpoint`: エンドポイント本体（`metadata` は `last_latency_ms` / `error_rate` / `consecutive_failures` を格納する柔軟な JSON）。
- `ServerView`: レスポンス用（`api_key` を除外した安全なビュー）。
- `RoutingDecision`: 選択されたサーバ id・理由・時刻。
- `AuditLog`: 監査ログ（id, action, user_id, details, timestamp）。

### 2.4 `storage/` — 永続化層
**トレイト**: `Storage`（`Send + Sync` + async）で抽象化。
```rust
trait Storage {
    get_server / list_servers / upsert_server / delete_server
    record_routing_decision
    record_audit_log / list_audit_logs
}
```
- **`InMemoryStorage`**: `RwLock<HashMap>` 等による開発用実装（再起動で消失）。
- **`PostgresStorage`**: `sqlx`（`PgPool`）で `servers` / `routing_history` / `audit_logs` を操作。
  - `with_crypt` で `Crypt` を注入すると `api_key` を AES-256-GCM で暗号化して保存、読み出し時に復号。暗号未注入なら平文。

### 2.5 `router/mod.rs` — ルーティングエンジン
`route(kind, strategy)` の処理フロー:
1. カウンタ `ROUTING_REQUESTS` をインクリメント。
2. `list_servers()` から `kind` 一致 かつ `Healthy` / `Degraded` かつ**サーキットブレーカー未開放**のもののみ抽出。
3. 該当が無ければエラー。
4. 戦略に応じて選択:
   - **Score**（既定）: `score = weight - latency/20 - error_rate*100 - failures*15 - health_penalty` で最大を選択。
   - **WeightedRandom**: weight に比例した確率でランダム。
   - **RoundRobin**: 均等に順番で選択。
5. 選択結果を `record_routing_decision` で履歴保存。

**サーキットブレーカー**: `metadata.consecutive_failures` が閾値（3）以上なら選択対象から除外。ヘルスチェック Worker が成功で 0 リセット、失敗でインクリメント。

`RoutingStrategy` は `FromStr` 対応で env / API 両方から指定可能。

### 2.6 `telemetry/mod.rs` — ヘルスチェック & メトリクス
- `prometheus` で `REGISTRY`、`ROUTING_REQUESTS`、`HEALTH_CHECK_FAILURE` を定義。
- `health_check_worker(storage)`: 60 秒周期で全サーバに対し `GET endpoint_url`（3 秒タイムアウト）。
  - 2xx → `Healthy`、その他応答 → `Degraded`、通信失敗 → `Unhealthy`。HTTP でない URL は `Unknown`。
  - `last_checked_at` / `last_latency_ms` / `consecutive_failures` を `metadata` に記録（サーキットブレーカー連動）。
- `metrics_handler()`: Prometheus フォーマット文字列を返す。

### 2.7 `auth/mod.rs` — 認証ミドルウェア + 権限
- `MANAGEMENT_API_TOKEN` → `Role::Admin`、`VIEWER_API_TOKEN` → `Role::Viewer` を判定し、`Role` と `UserIdentity`（マスク済みトークン）をリクエスト拡張に注入。
- `api::require_write` ミドルウェアが書き込み系ルートで Viewer を 403 に。

### 2.8 `api/mod.rs` — Management API（HTTP）
ルート一覧（すべて認証必須）:
| メソッド | パス | 機能 | 権限 |
| --- | --- | --- | --- |
| GET | `/servers` | エンドポイント一覧 | Admin / Viewer |
| POST | `/servers` | エンドポイント作成 | Admin |
| GET | `/servers/{id}` | 個別取得 | Admin / Viewer |
| DELETE | `/servers/{id}` | 削除 | Admin |
| POST | `/route` | ルーティング実行（戦略指定可・メッセージでフォワード） | Admin |
| GET | `/metrics` | Prometheus メトリクス | Admin / Viewer |
| GET | `/audit-logs` | 監査ログ一覧 | Admin / Viewer |

`POST /route` のボディ:
```json
{
  "kind": "llm",
  "message": "hello",          // 指定時のみプロバイダへフォワード
  "tool": "optional-tool",     // MCP の場合の tool 名
  "tool_arguments": { "...": "..." },
  "strategy": "score"          // 省略時は env 既定
}
```

### 2.9 `providers/` — プロバイダ実通信
- **`mod.rs`**: `forward(kind, endpoint_url, api_key, request)` のディスパッチャ（LLM / MCP 分岐）。
- **`llm.rs`**: OpenAI 互換 `/chat/completions` を `reqwest` で呼び出し、`choices[0].message.content` を返す。Bearer 認証対応。
- **`mcp.rs`**: rmcp の `StreamableHttpClientTransport`（streamable HTTP）で接続し `call_tool` を実行。`tool_arguments` に `message` を注入し、テキスト / structured_content を返す。

### 2.10 `security/mod.rs` — 保存時暗号化
- `Crypt`: `ENCRYPTION_KEY`（32 バイト hex）から `Aes256Gcm` を構築。
- `encrypt`: 平文 → base64(nonce(12B) || ciphertext)。
- `decrypt`: その逆。
- 非決定性（nonce 毎に異なる暗号文）と認証タグ検証を提供。

### 2.11 `tools/mod.rs` — MCP ツール & コマンド安全化
`rmcp` の `tool_router` + `#[tool]` で定義。`RustForgeServer::new(SafetyConfig)` で安全設定を注入。
- `ping` / `list_files` / `read_file` / `write_file` / `cargo` / `cargo_check` / `cargo_test` / `cargo_clippy` / `cargo_add`
- **パス制限**: ファイルツールは `resolve_path` でワークスペースルート外（`..` 等）を拒否。絶対パスもルート内のみ許可。
- **コマンド許可リスト**: `cargo` のサブコマンドが `CARGO_ALLOWED_COMMANDS` に無い場合は拒否。既定は `build/check/test/clippy/fmt/add/doc`（`publish`・`clean`・`owner` 等は拒否）。
- **シェルメタ文字検証**: `;|&`$()<>\n\r` を含む引数を拒否（インジェクション対策）。
- **タイムアウト**: `CARGO_TIMEOUT_SECS` を超えると子プロセスを kill（既定 300 秒）。

---

## 3. データベーススキーマ（migrations/20260513000000_init.sql）

- **`servers`**: id(UUID PK), name, kind, endpoint_url, api_key, health, weight, metadata(JSONB), created_at, updated_at
- **`audit_logs`**: id, action, user_id, details, timestamp（**API で記録・閲覧可能**）
- **`routing_history`**: id, selected_server_id(FK→servers), reason, timestamp

> スキーマ変更は不要。新機能（サーキットブレーカー、戦略、暗号化、監査）は `metadata`(JSONB) と既存列で実現しています。

---

## 4. 実装済み / 未実装 サマリ

| 機能 | 状態 |
| --- | --- |
| MCP ツール（ファイル/cargo/ping） | ✅ 実装済み |
| Storage（PostgreSQL / InMemory、DI 切替） | ✅ 実装済み |
| ルーティングエンジン（Score） | ✅ 実装済み |
| ルーティング戦略（WeightedRandom / RoundRobin） | ✅ 実装済み |
| サーキットブレーカー（consecutive_failures） | ✅ 実装済み |
| Management API（CRUD / route / metrics） | ✅ 実装済み |
| プロバイダ実通信（LLM / MCP） | ✅ 実装済み |
| ヘルスチェック Worker / Prometheus | ✅ 実装済み |
| 監査ログ（記録 / `GET /audit-logs`） | ✅ 実装済み |
| Bearer token 認証 + RBAC（Admin / Viewer） | ✅ 実装済み |
| コマンド安全化（許可リスト / パス制限 / タイムアウト） | ✅ 実装済み |
| API キー保存時暗号化（AES-256-GCM） | ✅ 実装済み |

### 今後（未着手・将来課題）
| 領域 | 内容 |
| --- | --- |
| MCP プロバイダの stdio/SSE 対応 | 現状 streamable HTTP のみ。他のトランスポート対応 |
| 暗号化キーのローテーション | キー更新時の再暗号化運用 |
| 監査 API のフィルタ/ページング強化 | 現状 limit のみ |
| ルーティングのリクエスト遂行結果フィードバック | 呼び出し成功/失敗の自動反映 |
