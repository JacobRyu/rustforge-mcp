use anyhow::{Context, Result, anyhow};
use serde_json::json;

/// OpenAI 互換の chat completions API を呼び出し、応答テキストを返す。
pub async fn call_llm(
    http: &reqwest::Client,
    endpoint_url: &str,
    api_key: Option<&str>,
    message: &str,
) -> Result<String> {
    let url = normalize_chat_url(endpoint_url);
    let mut builder = http.post(url).json(&json!({
        "model": "default",
        "messages": [
            { "role": "user", "content": message }
        ]
    }));

    if let Some(key) = api_key {
        builder = builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {key}"));
    }

    let response = builder.send().await.context("failed to send LLM request")?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.context("failed to parse LLM response")?;

    if !status.is_success() {
        return Err(anyhow!("LLM provider returned {status}: {body}"));
    }

    let text = body
        .pointer("/choices/0/message/content")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("LLM response missing choices[0].message.content: {body}"))?;

    Ok(text.to_string())
}

/// `/chat/completions` をエンドポイント先頭につける。
fn normalize_chat_url(endpoint_url: &str) -> String {
    let trimmed = endpoint_url.trim_end_matches('/');
    match trimmed {
        s if s.ends_with("/chat/completions") => s.to_string(),
        s => format!("{s}/chat/completions"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_chat_url_appends_path() {
        assert_eq!(
            normalize_chat_url("http://localhost:8080"),
            "http://localhost:8080/chat/completions"
        );
    }

    #[test]
    fn normalize_chat_url_keeps_existing_path() {
        assert_eq!(
            normalize_chat_url("http://localhost/v1/chat/completions"),
            "http://localhost/v1/chat/completions"
        );
    }

    #[tokio::test]
    async fn call_llm_parses_openai_response() {
        use axum::Router;
        use axum::routing::post;

        let app = Router::new().route(
            "/chat/completions",
            post(|payload: axum::Json<serde_json::Value>| async move {
                assert!(payload.0.get("messages").is_some());
                axum::Json(json!({
                    "choices": [{ "message": { "content": "hello world" } }]
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let http = reqwest::Client::new();
        let out = call_llm(&http, &format!("http://{addr}"), None, "hi").await.unwrap();
        assert_eq!(out, "hello world");
    }
}
