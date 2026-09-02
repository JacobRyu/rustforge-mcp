use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::Response,
};

use crate::api::{AppState, Role};
use std::sync::Arc;

/// 認証済みユーザの識別情報（リクエスト拡張として注入される）
#[derive(Debug, Clone)]
pub struct UserIdentity(pub String);

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let candidate =
        req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).map(str::to_string);

    let (role, token) = if let Some(auth_str) = candidate {
        if let Some(token) = auth_str.strip_prefix("Bearer ") {
            if token == state.management_api_token {
                (Role::Admin, token.to_string())
            } else if Some(token) == state.viewer_api_token.as_deref() {
                (Role::Viewer, token.to_string())
            } else {
                return Err(StatusCode::UNAUTHORIZED);
            }
        } else if auth_str == state.management_api_token {
            (Role::Admin, auth_str)
        } else {
            return Err(StatusCode::UNAUTHORIZED);
        }
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    req.extensions_mut().insert(role);
    req.extensions_mut().insert(UserIdentity(mask(&token)));
    Ok(next.run(req).await)
}

fn mask(token: &str) -> String {
    if token.len() <= 8 { "token:********".to_string() } else { format!("token:{}", &token[..8]) }
}

impl<S: Send + Sync> FromRequestParts<S> for UserIdentity {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<UserIdentity>().cloned().ok_or(StatusCode::UNAUTHORIZED)
    }
}
