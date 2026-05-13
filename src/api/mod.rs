use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use crate::models::{HealthState, ProviderKind, ServerEndpoint, ServerView};
use crate::storage::Storage;
use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;
use serde::Deserialize;

use crate::router::RoutingEngine;

pub struct AppState {
    pub storage: Arc<dyn Storage>,
    pub router: Arc<RoutingEngine>,
    pub management_api_token: String,
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
}

use axum::middleware;
use crate::auth::auth_middleware;

use crate::telemetry::metrics_handler;

pub fn app(state: Arc<AppState>) -> Router {
    let middleware_state = state.clone();

    Router::new()
        .route("/servers", get(list_servers).post(create_server))
        .route("/servers/:id", get(get_server).delete(delete_server))
        .route("/route", post(route_request))
        .route("/metrics", get(get_metrics))
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
        Ok(_) => (
            StatusCode::CREATED,
            Json(ServerView::from(&server)),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_server(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.storage.get_server(id).await {
        Ok(Some(server)) => (StatusCode::OK, Json(ServerView::from(&server))).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_server(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.storage.delete_server(id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn route_request(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RouteRequest>,
) -> impl IntoResponse {
    match state.router.route(payload.kind).await {
        Ok(decision) => (StatusCode::OK, Json(decision)).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e.to_string()).into_response(),
    }
}
