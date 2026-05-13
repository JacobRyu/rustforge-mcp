mod api;
mod auth;
mod config;
mod models;
mod providers;
mod router;
mod storage;
mod telemetry;
mod tools;

use std::sync::Arc;
use rmcp::ServiceExt;
use tools::RustForgeServer;
use crate::api::AppState;
use crate::config::AppConfig;
use crate::storage::{InMemoryStorage, PostgresStorage, Storage};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let config = AppConfig::from_env()?;

    let storage: Arc<dyn Storage> = if config.use_in_memory_storage {
        tracing::warn!("Using in-memory storage; data will not persist across restarts");
        Arc::new(InMemoryStorage::new())
    } else {
        let database_url = config
            .database_url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("DATABASE_URL is required"))?;
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(&database_url)
            .await?;
        Arc::new(PostgresStorage::new(pool))
    };

    let router = Arc::new(router::RoutingEngine::new(storage.clone()));
    let state = Arc::new(AppState {
        storage: storage.clone(),
        router,
        management_api_token: config.management_api_token.clone(),
    });

    // Start Management API in the background
    let app = api::app(state);
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("Management API listening on {}", listener.local_addr()?);
    
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("Management API server exited with error: {}", e);
        }
    });

    // Start Health Check Worker
    let storage_for_worker = storage.clone();
    tokio::spawn(async move {
        telemetry::health_check_worker(storage_for_worker).await;
    });

    // Start the MCP server using stdio transport
    // Note: Since this uses stdin/stdout, it will take over the terminal.
    // In a real control plane, you might want to run this as a client or a different transport.
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let service = RustForgeServer.serve(transport).await?;
    tracing::info!("MCP Server started on stdio");
    service.waiting().await?;
    
    Ok(())
}
