use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};

use std::sync::Arc;
use crate::api::AppState;

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = req.headers().get(header::AUTHORIZATION);

    if let Some(auth_value) = auth_header {
        if let Ok(auth_str) = auth_value.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                if token == state.management_api_token {
                    return Ok(next.run(req).await);
                }
            }
            if auth_str == state.management_api_token {
                return Ok(next.run(req).await);
            }
        }
    }

    tracing::warn!("Unauthorized access attempt");
    Err(StatusCode::UNAUTHORIZED)
}
