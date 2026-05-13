use crate::models::{HealthState, ProviderKind, RoutingDecision, ServerEndpoint};
use crate::storage::Storage;
use sqlx::{PgPool, Row};
use uuid::Uuid;
use async_trait::async_trait;
use anyhow::{anyhow, Result};

pub struct PostgresStorage {
    pool: PgPool,
}

impl PostgresStorage {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn parse_provider_kind(raw: &str) -> Result<ProviderKind> {
    match raw {
        "mcp" => Ok(ProviderKind::Mcp),
        "llm" => Ok(ProviderKind::Llm),
        _ => Err(anyhow!("invalid provider kind: {raw}")),
    }
}

fn parse_health(raw: &str) -> Result<HealthState> {
    match raw {
        "healthy" => Ok(HealthState::Healthy),
        "unhealthy" => Ok(HealthState::Unhealthy),
        "degraded" => Ok(HealthState::Degraded),
        "unknown" => Ok(HealthState::Unknown),
        _ => Err(anyhow!("invalid health state: {raw}")),
    }
}

fn provider_kind_as_str(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Mcp => "mcp",
        ProviderKind::Llm => "llm",
    }
}

fn health_as_str(health: &HealthState) -> &'static str {
    match health {
        HealthState::Healthy => "healthy",
        HealthState::Unhealthy => "unhealthy",
        HealthState::Degraded => "degraded",
        HealthState::Unknown => "unknown",
    }
}

#[async_trait]
impl Storage for PostgresStorage {
    async fn get_server(&self, id: Uuid) -> anyhow::Result<Option<ServerEndpoint>> {
        let row = sqlx::query(
            "SELECT id, name, kind, endpoint_url, api_key, health, weight, metadata, created_at, updated_at FROM servers WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(ServerEndpoint {
                id: row.get("id"),
                name: row.get("name"),
                kind: parse_provider_kind(row.get("kind"))?,
                endpoint_url: row.get("endpoint_url"),
                api_key: row.get("api_key"),
                health: parse_health(row.get("health"))?,
                weight: row.get::<i32, _>("weight") as u32,
                metadata: row.get("metadata"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    async fn list_servers(&self) -> anyhow::Result<Vec<ServerEndpoint>> {
        let rows = sqlx::query(
            "SELECT id, name, kind, endpoint_url, api_key, health, weight, metadata, created_at, updated_at FROM servers"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut servers = Vec::new();
        for row in rows {
            servers.push(ServerEndpoint {
                id: row.get("id"),
                name: row.get("name"),
                kind: parse_provider_kind(row.get("kind"))?,
                endpoint_url: row.get("endpoint_url"),
                api_key: row.get("api_key"),
                health: parse_health(row.get("health"))?,
                weight: row.get::<i32, _>("weight") as u32,
                metadata: row.get("metadata"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }
        Ok(servers)
    }

    async fn upsert_server(&self, server: ServerEndpoint) -> anyhow::Result<()> {
        sqlx::query(
            r#"INSERT INTO servers (id, name, kind, endpoint_url, api_key, health, weight, metadata, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               ON CONFLICT (id) DO UPDATE SET
               name = EXCLUDED.name, kind = EXCLUDED.kind, endpoint_url = EXCLUDED.endpoint_url,
               api_key = EXCLUDED.api_key, health = EXCLUDED.health, weight = EXCLUDED.weight,
               metadata = EXCLUDED.metadata, updated_at = EXCLUDED.updated_at"#
        )
        .bind(server.id)
        .bind(server.name)
        .bind(provider_kind_as_str(&server.kind))
        .bind(server.endpoint_url)
        .bind(server.api_key)
        .bind(health_as_str(&server.health))
        .bind(server.weight as i32)
        .bind(server.metadata)
        .bind(server.created_at)
        .bind(server.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_server(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM servers WHERE id = $1")
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_routing_decision(&self, decision: RoutingDecision) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO routing_history (id, selected_server_id, reason, timestamp) VALUES ($1, $2, $3, $4)"
        )
        .bind(Uuid::new_v4())
        .bind(decision.selected_server_id)
        .bind(decision.reason)
        .bind(decision.timestamp)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
