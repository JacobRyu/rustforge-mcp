use anyhow::{Result, anyhow};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, JsonObject};
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};

/// MCP サーバの tool を streamable HTTP 経由で呼び出し、結果テキストを返す。
pub async fn call_mcp(
    http: reqwest::Client,
    endpoint_url: &str,
    api_key: Option<&str>,
    tool: Option<&str>,
    tool_arguments: Option<&serde_json::Value>,
    message: &str,
) -> Result<String> {
    let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint_url);
    if let Some(key) = api_key {
        config = config.auth_header(key);
    }

    let transport = StreamableHttpClientTransport::with_client(http, config);
    let client = ().serve(transport).await?;

    let tool_name = tool.unwrap_or("chat").to_string();
    let mut arguments: JsonObject =
        tool_arguments.and_then(serde_json::Value::as_object).cloned().unwrap_or_default();
    arguments.insert("message".to_string(), serde_json::json!(message));

    let result =
        client.call_tool(CallToolRequestParams::new(tool_name).with_arguments(arguments)).await?;

    render_mcp_result(result)
}

fn render_mcp_result(result: CallToolResult) -> Result<String> {
    if result.is_error == Some(true) {
        return Err(anyhow!("MCP tool returned an error result"));
    }

    let text: Vec<String> = result
        .content
        .into_iter()
        .filter_map(|c| {
            use rmcp::model::RawContent;
            match std::ops::Deref::deref(&c) {
                RawContent::Text(t) => Some(t.text.clone()),
                _ => None,
            }
        })
        .collect();

    if text.is_empty() {
        if let Some(structured) = result.structured_content {
            return Ok(structured.to_string());
        }
        return Err(anyhow!("MCP tool returned no textual content"));
    }

    Ok(text.join("\n"))
}
