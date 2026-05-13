use crate::models::{ServerEndpoint, HealthState, RoutingDecision, ProviderKind};
use crate::storage::Storage;
use crate::telemetry::ROUTING_REQUESTS;
use std::sync::Arc;
use chrono::Utc;

pub struct RoutingEngine {
    storage: Arc<dyn Storage>,
}

impl RoutingEngine {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }

    pub async fn route(&self, kind: ProviderKind) -> anyhow::Result<RoutingDecision> {
        ROUTING_REQUESTS.inc();
        let servers = self.storage.list_servers().await?;
        
        let healthy_servers: Vec<ServerEndpoint> = servers
            .into_iter()
            .filter(|s| {
                s.kind == kind
                    && (s.health == HealthState::Healthy || s.health == HealthState::Degraded)
            })
            .collect();

        if healthy_servers.is_empty() {
            return Err(anyhow::anyhow!("No healthy servers available for {:?}", kind));
        }

        let selected = healthy_servers
            .iter()
            .max_by(|a, b| route_score(a).total_cmp(&route_score(b)))
            .ok_or_else(|| anyhow::anyhow!("Failed to select a server"))?;

        let decision = RoutingDecision {
            selected_server_id: selected.id,
            reason: format!(
                "Selected '{}' by score {:.2} (weight={}, health={:?})",
                selected.name,
                route_score(selected),
                selected.weight,
                selected.health
            ),
            timestamp: Utc::now(),
        };

        self.storage.record_routing_decision(decision.clone()).await?;
        Ok(decision)
    }
}

fn route_score(server: &ServerEndpoint) -> f64 {
    let latency_ms = server
        .metadata
        .get("last_latency_ms")
        .and_then(|v| v.as_f64())
        .unwrap_or(500.0);
    let error_rate = server
        .metadata
        .get("error_rate")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let health_penalty = match server.health {
        HealthState::Healthy => 0.0,
        HealthState::Degraded => 40.0,
        HealthState::Unknown => 80.0,
        HealthState::Unhealthy => 120.0,
    };

    f64::from(server.weight) - (latency_ms / 20.0) - (error_rate * 100.0) - health_penalty
}
