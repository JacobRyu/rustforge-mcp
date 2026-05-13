use crate::models::{RoutingDecision, ServerEndpoint};
use uuid::Uuid;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod postgres;
pub use postgres::PostgresStorage;

#[async_trait::async_trait]
pub trait Storage: Send + Sync {
    async fn get_server(&self, id: Uuid) -> anyhow::Result<Option<ServerEndpoint>>;
    async fn list_servers(&self) -> anyhow::Result<Vec<ServerEndpoint>>;
    async fn upsert_server(&self, server: ServerEndpoint) -> anyhow::Result<()>;
    async fn delete_server(&self, id: Uuid) -> anyhow::Result<()>;
    async fn record_routing_decision(&self, decision: RoutingDecision) -> anyhow::Result<()>;
}

pub struct InMemoryStorage {
    servers: Arc<RwLock<HashMap<Uuid, ServerEndpoint>>>,
    routing_history: Arc<RwLock<Vec<RoutingDecision>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            routing_history: Arc::new(RwLock::new(Vec::new())),
        }
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
}
