use crate::models::{AuditLog, HealthState, ProviderKind, RoutingDecision, ServerEndpoint};
use crate::security::Crypt;
use crate::storage::Storage;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct PostgresStorage {
    pool: PgPool,
    crypt: Option<Crypt>,
}

impl PostgresStorage {
    pub fn with_crypt(pool: PgPool, crypt: Option<Crypt>) -> Self {
        Self { pool, crypt }
    }

    fn encrypt_api_key(&self, api_key: &Option<String>) -> Result<Option<String>> {
        match (api_key, &self.crypt) {
            (None, _) => Ok(None),
            (Some(_), None) => Ok(api_key.clone()),
            (Some(key), Some(crypt)) => Ok(Some(crypt.encrypt(key)?)),
        }
    }

    fn decrypt_api_key(&self, api_key: &Option<String>) -> Result<Option<String>> {
        match (api_key, &self.crypt) {
            (None, _) => Ok(None),
            (Some(_), None) => Ok(api_key.clone()),
            (Some(enc), Some(crypt)) => Ok(Some(crypt.decrypt(enc)?)),
        }
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
            let api_key_enc: Option<String> = row.get("api_key");
            Ok(Some(ServerEndpoint {
                id: row.get("id"),
                name: row.get("name"),
                kind: parse_provider_kind(row.get("kind"))?,
                endpoint_url: row.get("endpoint_url"),
                api_key: self.decrypt_api_key(&api_key_enc)?,
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
            let api_key_enc: Option<String> = row.get("api_key");
            servers.push(ServerEndpoint {
                id: row.get("id"),
                name: row.get("name"),
                kind: parse_provider_kind(row.get("kind"))?,
                endpoint_url: row.get("endpoint_url"),
                api_key: self.decrypt_api_key(&api_key_enc)?,
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
        .bind(self.encrypt_api_key(&server.api_key)?)
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
        sqlx::query("DELETE FROM servers WHERE id = $1").bind(id).execute(&self.pool).await?;
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

    async fn record_audit_log(&self, log: AuditLog) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO audit_logs (id, action, user_id, details, timestamp) VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(log.id)
        .bind(log.action)
        .bind(log.user_id)
        .bind(log.details)
        .bind(log.timestamp)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn list_audit_logs(&self, limit: Option<u32>) -> anyhow::Result<Vec<AuditLog>> {
        let limit = limit.unwrap_or(100).max(1) as i64;
        let rows = sqlx::query(
            "SELECT id, action, user_id, details, timestamp FROM audit_logs ORDER BY timestamp DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut logs = Vec::new();
        for row in rows {
            logs.push(AuditLog {
                id: row.get("id"),
                action: row.get("action"),
                user_id: row.get("user_id"),
                details: row.get("details"),
                timestamp: row.get("timestamp"),
            });
        }
        Ok(logs)
    }
}
