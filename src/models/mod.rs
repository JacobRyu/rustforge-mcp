use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Mcp,
    Llm,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Healthy,
    Unhealthy,
    Degraded,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEndpoint {
    pub id: Uuid,
    pub name: String,
    pub kind: ProviderKind,
    pub endpoint_url: String,
    pub api_key: Option<String>,
    pub health: HealthState,
    pub weight: u32,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerView {
    pub id: Uuid,
    pub name: String,
    pub kind: ProviderKind,
    pub endpoint_url: String,
    pub health: HealthState,
    pub weight: u32,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&ServerEndpoint> for ServerView {
    fn from(value: &ServerEndpoint) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
            kind: value.kind.clone(),
            endpoint_url: value.endpoint_url.clone(),
            health: value.health.clone(),
            weight: value.weight,
            metadata: value.metadata.clone(),
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub selected_server_id: Uuid,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: Uuid,
    pub action: String,
    pub user_id: Option<String>,
    pub details: String,
    pub timestamp: DateTime<Utc>,
}
