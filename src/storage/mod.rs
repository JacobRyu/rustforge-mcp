use crate::models::{AuditLog, RoutingDecision, ServerEndpoint};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub mod postgres;
pub use postgres::PostgresStorage;

#[async_trait::async_trait]
pub trait Storage: Send + Sync {
    async fn get_server(&self, id: Uuid) -> anyhow::Result<Option<ServerEndpoint>>;
    async fn list_servers(&self) -> anyhow::Result<Vec<ServerEndpoint>>;
    async fn upsert_server(&self, server: ServerEndpoint) -> anyhow::Result<()>;
    async fn delete_server(&self, id: Uuid) -> anyhow::Result<()>;
    async fn record_routing_decision(&self, decision: RoutingDecision) -> anyhow::Result<()>;
    async fn record_audit_log(&self, log: AuditLog) -> anyhow::Result<()>;
    async fn list_audit_logs(&self, limit: Option<u32>) -> anyhow::Result<Vec<AuditLog>>;
}

pub struct InMemoryStorage {
    servers: Arc<RwLock<HashMap<Uuid, ServerEndpoint>>>,
    routing_history: Arc<RwLock<Vec<RoutingDecision>>>,
    audit_logs: Arc<RwLock<Vec<AuditLog>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            routing_history: Arc::new(RwLock::new(Vec::new())),
            audit_logs: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Storage for InMemoryStorage {
    async fn get_server(&self, id: Uuid) -> anyhow::Result<Option<ServerEndpoint>> {
        let servers = self.servers.read().await;
        Ok(servers.get(&id).cloned())
    }

    async fn list_servers(&self) -> anyhow::Result<Vec<ServerEndpoint>> {
        let servers = self.servers.read().await;
        Ok(servers.values().cloned().collect())
    }

    async fn upsert_server(&self, server: ServerEndpoint) -> anyhow::Result<()> {
        let mut servers = self.servers.write().await;
        servers.insert(server.id, server);
        Ok(())
    }

    async fn delete_server(&self, id: Uuid) -> anyhow::Result<()> {
        let mut servers = self.servers.write().await;
        servers.remove(&id);
        Ok(())
    }

    async fn record_routing_decision(&self, decision: RoutingDecision) -> anyhow::Result<()> {
        let mut history = self.routing_history.write().await;
        history.push(decision);
        Ok(())
    }

    async fn record_audit_log(&self, log: AuditLog) -> anyhow::Result<()> {
        let mut logs = self.audit_logs.write().await;
        logs.push(log);
        Ok(())
    }

    async fn list_audit_logs(&self, limit: Option<u32>) -> anyhow::Result<Vec<AuditLog>> {
        let logs = self.audit_logs.read().await;
        let limit = limit.unwrap_or(u32::MAX) as usize;
        let slice: Vec<AuditLog> = logs.iter().rev().take(limit).cloned().collect();
        Ok(slice)
    }
}
