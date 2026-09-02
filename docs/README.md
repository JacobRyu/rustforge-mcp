# rustforge-mcp ドキュメント

プロジェクトの分析に基づく技術ドキュメント一式です。

| ドキュメント | 内容 |
| --- | --- |
| [ROADMAP.md](./ROADMAP.md) | **完成図（ビジョン）** — 最終的に目指す姿、全体構成、将来機能、進化の方向性 |
| [IMPLEMENTATION.md](./IMPLEMENTATION.md) | **現状の実装内容** — モジュール単位の実装詳細、実装済み/未実装サマリ |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | **設計意図と理由** — なぜこの設計を選んだのか、トレードオフと残課題 |
| [PLAN.md](./PLAN.md) | **実装計画** — 未実装部分を 5 フェーズで進める計画と進捗 |

## 読み方の推奨

1. まず [ROADMAP.md](./ROADMAP.md) で全体像・目的（完成図）を把握。
2. 次に [IMPLEMENTATION.md](./IMPLEMENTATION.md) で現状何が実装されているか確認。
3. 最後に [ARCHITECTURE.md](./ARCHITECTURE.md) で各設計判断の理由を掘り下げる。

## プロジェクト概要（要約）

rustforge-mcp は Rust 開発用の MCP サーバであり、同時に複数 MCP / LLM プロバイダをルーティングするゲートウェイ基盤です。MCP ツール（ファイル操作 / cargo）、storage / router / API / telemetry / auth の各基盤に加え、プロバイダ実通信（LLM・MCP streamable HTTP）、監査ログ API、3 つのルーティング戦略 + サーキットブレーカー、コマンド安全化、API キーの保存時暗号化まで実装されています。

全 5 フェーズの実装は完了し、`cargo fmt --check` / `cargo clippy -D warnings` / `cargo test`（19 件）で検証済みです。
