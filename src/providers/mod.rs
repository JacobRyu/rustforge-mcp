// Providers 連携モジュール
//
// ルーティングエンジンが選んだエンドポイントに対して実際にリクエストを
// フォワードするクライアント群を提供する。
//
// - LLM: OpenAI 互換の chat completions API を呼び出す
// - MCP: rmcp の streamable HTTP クライアントでサーバへ接続し tool を呼び出す

pub mod llm;
pub mod mcp;

use crate::models::ProviderKind;
use anyhow::Result;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub struct ProviderRequest<'a> {
    pub message: &'a str,
    pub tool: Option<&'a str>,
    pub tool_arguments: Option<&'a serde_json::Value>,
}

pub async fn forward(
    kind: ProviderKind,
    endpoint_url: &str,
    api_key: Option<&str>,
    request: ProviderRequest<'_>,
) -> Result<String> {
    let http = reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build()?;

    match kind {
        ProviderKind::Llm => llm::call_llm(&http, endpoint_url, api_key, request.message).await,
        ProviderKind::Mcp => {
            mcp::call_mcp(
                http,
                endpoint_url,
                api_key,
                request.tool,
                request.tool_arguments,
                request.message,
            )
            .await
        }
    }
}
