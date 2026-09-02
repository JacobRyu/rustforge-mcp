use crate::auth::{UserIdentity, auth_middleware};
use crate::models::{AuditLog, HealthState, ProviderKind, ServerEndpoint, ServerView};
use crate::providers::{self, ProviderRequest};
use crate::router::RoutingEngine;
use crate::storage::Storage;
use crate::telemetry::metrics_handler;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

pub struct AppState {
    pub storage: Arc<dyn Storage>,
    pub router: Arc<RoutingEngine>,
    pub management_api_token: String,
    pub viewer_api_token: Option<String>,
    pub default_strategy: crate::router::RoutingStrategy,
}

/// 認証されたクライアントの権限
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Role {
    Admin,
    Viewer,
}

/// 書き込み操作の権限チェック（Viewer は読取のみ許可）
async fn require_write(req: axum::extract::Request, next: Next) -> Result<Response, StatusCode> {
    let role = req.extensions().get::<Role>().copied().unwrap_or(Role::Viewer);
    match role {
        Role::Admin => Ok(next.run(req).await),
        Role::Viewer => Err(StatusCode::FORBIDDEN),
    }
}

#[derive(Deserialize)]
pub struct CreateServerRequest {
    pub name: String,
    pub kind: ProviderKind,
    pub endpoint_url: String,
    pub api_key: Option<String>,
    pub weight: Option<u32>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct RouteRequest {
    pub kind: ProviderKind,
    pub message: Option<String>,
    pub tool: Option<String>,
    pub tool_arguments: Option<serde_json::Value>,
    pub strategy: Option<crate::router::RoutingStrategy>,
}

#[derive(Deserialize)]
pub struct AuditLogQuery {
    pub limit: Option<u32>,
}

pub fn app(state: Arc<AppState>) -> Router {
    let middleware_state = state.clone();

    Router::new()
        .route("/servers", get(list_servers))
        .route("/servers/{id}", get(get_server))
        .route("/metrics", get(get_metrics))
        .route("/audit-logs", get(list_audit_logs))
        .merge(
            Router::new()
                .route("/servers", post(create_server))
                .route("/servers/{id}", axum::routing::delete(delete_server))
                .route("/route", post(route_request))
                .layer(middleware::from_fn(require_write)),
        )
        .layer(middleware::from_fn_with_state(middleware_state, auth_middleware))
        .with_state(state)
}

async fn get_metrics() -> impl IntoResponse {
    metrics_handler()
}

async fn list_servers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.storage.list_servers().await {
        Ok(servers) => {
            let response: Vec<ServerView> = servers.iter().map(ServerView::from).collect();
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn create_server(
    State(state): State<Arc<AppState>>,
    user: UserIdentity,
    Json(payload): Json<CreateServerRequest>,
) -> impl IntoResponse {
    let server = ServerEndpoint {
        id: Uuid::new_v4(),
        name: payload.name,
        kind: payload.kind,
        endpoint_url: payload.endpoint_url,
        api_key: payload.api_key,
        health: HealthState::Unknown,
        weight: payload.weight.unwrap_or(100),
        metadata: payload.metadata.unwrap_or(serde_json::json!({})),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    match state.storage.upsert_server(server.clone()).await {
        Ok(_) => {
            record_audit(&state, "server.create", &user, &format!("id={}", server.id)).await;
            (StatusCode::CREATED, Json(ServerView::from(&server))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_server(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match state.storage.get_server(id).await {
        Ok(Some(server)) => (StatusCode::OK, Json(ServerView::from(&server))).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_server(
    State(state): State<Arc<AppState>>,
    user: UserIdentity,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.storage.delete_server(id).await {
        Ok(_) => {
            record_audit(&state, "server.delete", &user, &format!("id={id}")).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_audit_logs(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<AuditLogQuery>,
) -> impl IntoResponse {
    match state.storage.list_audit_logs(query.limit).await {
        Ok(logs) => (StatusCode::OK, Json(logs)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn route_request(
    State(state): State<Arc<AppState>>,
    user: UserIdentity,
    Json(payload): Json<RouteRequest>,
) -> impl IntoResponse {
    let kind = payload.kind;
    let kind_str = format!("{kind:?}");
    let strategy = payload.strategy.unwrap_or(state.default_strategy);
    let decision = match state.router.route(kind, strategy).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::SERVICE_UNAVAILABLE, e.to_string()).into_response(),
    };

    record_audit(
        &state,
        "route.request",
        &user,
        &format!(
            "kind={kind_str}, strategy={strategy:?}, selected={}",
            decision.selected_server_id
        ),
    )
    .await;

    // フォワード対象のエンドポイントを取得
    let server = match state.storage.get_server(decision.selected_server_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Selected server no longer exists".to_string(),
            )
                .into_response();
        }
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // メッセージ指定があればプロバイダへ実際にフォワード
    let forwarded = match payload.message {
        Some(message) => {
            let request = ProviderRequest {
                message: &message,
                tool: payload.tool.as_deref(),
                tool_arguments: payload.tool_arguments.as_ref(),
            };
            Some(
                providers::forward(
                    server.kind,
                    &server.endpoint_url,
                    server.api_key.as_deref(),
                    request,
                )
                .await,
            )
        }
        None => None,
    };

    match forwarded {
        Some(Ok(response_text)) => {
            let response = RouteResponse {
                selected_server_id: decision.selected_server_id,
                reason: decision.reason,
                response: Some(response_text),
                timestamp: decision.timestamp,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Some(Err(e)) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
        None => {
            // フォワード無し（ルーティング決定のみ）
            let response = RouteResponse {
                selected_server_id: decision.selected_server_id,
                reason: decision.reason,
                response: None,
                timestamp: decision.timestamp,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
    }
}

#[derive(Serialize)]
struct RouteResponse {
    selected_server_id: Uuid,
    reason: String,
    response: Option<String>,
    timestamp: chrono::DateTime<chrono::Utc>,
}

async fn record_audit(state: &AppState, action: &str, user: &UserIdentity, details: &str) {
    let log = AuditLog {
        id: Uuid::new_v4(),
        action: action.to_string(),
        user_id: Some(user.0.clone()),
        details: details.to_string(),
        timestamp: Utc::now(),
    };
    if let Err(e) = state.storage.record_audit_log(log).await {
        tracing::error!("Failed to record audit log ({action}): {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InMemoryStorage;
    use tower::ServiceExt;

    fn test_app(admin_token: &str, viewer_token: Option<&str>) -> Router {
        let storage: Arc<dyn Storage> = Arc::new(InMemoryStorage::new());
        let router = Arc::new(RoutingEngine::new(storage.clone()));
        let state = Arc::new(AppState {
            storage,
            router,
            management_api_token: admin_token.to_string(),
            viewer_api_token: viewer_token.map(str::to_string),
            default_strategy: crate::router::RoutingStrategy::default(),
        });
        app(state)
    }

    async fn get(router: &Router, path: &str, token: &str) -> axum::response::Response {
        router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(path)
                    .header("authorization", token)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn post_json(
        router: &Router,
        path: &str,
        token: &str,
        body: serde_json::Value,
    ) -> axum::response::Response {
        router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", token)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn resp_bytes(response: axum::response::Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024).await.unwrap().to_vec()
    }

    #[tokio::test]
    async fn create_list_delete_audit_logs() {
        let router = test_app("admin-secret", None);

        let resp = post_json(
            &router,
            "/servers",
            "Bearer admin-secret",
            serde_json::json!({
                "name": "local-llm",
                "kind": "llm",
                "endpoint_url": "http://127.0.0.1:9999",
                "weight": 100
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = get(&router, "/audit-logs", "Bearer admin-secret").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let text = String::from_utf8(resp_bytes(resp).await).unwrap();
        assert!(text.contains("server.create"), "log body: {text}");
    }

    #[tokio::test]
    async fn viewer_cannot_write_but_can_read() {
        let router = test_app("admin-secret", Some("viewer-secret"));

        // Viewer による一覧参照は許可
        let resp = get(&router, "/servers", "Bearer viewer-secret").await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Viewer による書き込みは 403
        let resp = post_json(
            &router,
            "/servers",
            "Bearer viewer-secret",
            serde_json::json!({
                "name": "x",
                "kind": "llm",
                "endpoint_url": "http://127.0.0.1:9999"
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 不正トークンは 401
        let resp = get(&router, "/servers", "Bearer wrong-token").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
