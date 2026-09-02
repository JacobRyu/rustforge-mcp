use crate::models::{HealthState, ProviderKind, RoutingDecision, ServerEndpoint};
use crate::storage::Storage;
use crate::telemetry::ROUTING_REQUESTS;
use chrono::Utc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// ルーティング戦略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    /// health + weight + latency のスコアリングで最良を選ぶ（既定）
    #[default]
    Score,
    /// weight に比例した確率でランダム選択
    WeightedRandom,
    /// 均等に順番で選択
    RoundRobin,
}

impl std::str::FromStr for RoutingStrategy {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "score" => Ok(RoutingStrategy::Score),
            "weighted_random" => Ok(RoutingStrategy::WeightedRandom),
            "round_robin" => Ok(RoutingStrategy::RoundRobin),
            other => Err(anyhow::anyhow!("unknown routing strategy: {other}")),
        }
    }
}

/// 連続失敗によるサーキットブレーカー閾値
const CIRCUIT_BREAKER_THRESHOLD: u64 = 3;
/// metadata 上の失敗回数キー
pub const FAILURES_KEY: &str = "consecutive_failures";

pub struct RoutingEngine {
    storage: Arc<dyn Storage>,
    round_robin_index: AtomicUsize,
}

impl RoutingEngine {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage, round_robin_index: AtomicUsize::new(0) }
    }

    pub async fn route(
        &self,
        kind: ProviderKind,
        strategy: RoutingStrategy,
    ) -> anyhow::Result<RoutingDecision> {
        ROUTING_REQUESTS.inc();
        let servers = self.storage.list_servers().await?;

        let healthy_servers: Vec<ServerEndpoint> = servers
            .into_iter()
            .filter(|s| {
                s.kind == kind
                    && (s.health == HealthState::Healthy || s.health == HealthState::Degraded)
                    && !is_open_circuit(s)
            })
            .collect();

        if healthy_servers.is_empty() {
            return Err(anyhow::anyhow!("No healthy servers available for {:?}", kind));
        }

        self.select(&healthy_servers, strategy).await
    }

    async fn select(
        &self,
        servers: &[ServerEndpoint],
        strategy: RoutingStrategy,
    ) -> anyhow::Result<RoutingDecision> {
        match strategy {
            RoutingStrategy::Score => self.select_score(servers).await,
            RoutingStrategy::WeightedRandom => self.select_weighted_random(servers).await,
            RoutingStrategy::RoundRobin => self.select_round_robin(servers).await,
        }
    }

    async fn select_score(&self, servers: &[ServerEndpoint]) -> anyhow::Result<RoutingDecision> {
        let selected = servers
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

    async fn select_weighted_random(
        &self,
        servers: &[ServerEndpoint],
    ) -> anyhow::Result<RoutingDecision> {
        let total_weight: u64 = servers.iter().map(|s| u64::from(s.weight.max(1))).sum();
        let mut pick = rand::random::<u64>() % total_weight;

        let mut selected = &servers[0];
        for server in servers {
            let w = u64::from(server.weight.max(1));
            if pick < w {
                selected = server;
                break;
            }
            pick -= w;
        }

        let decision = RoutingDecision {
            selected_server_id: selected.id,
            reason: format!(
                "Weighted-random selected '{}' (weight={}, health={:?})",
                selected.name, selected.weight, selected.health
            ),
            timestamp: Utc::now(),
        };

        self.storage.record_routing_decision(decision.clone()).await?;
        Ok(decision)
    }

    async fn select_round_robin(
        &self,
        servers: &[ServerEndpoint],
    ) -> anyhow::Result<RoutingDecision> {
        let index = self.round_robin_index.fetch_add(1, Ordering::Relaxed) % servers.len();
        let selected = &servers[index];

        let decision = RoutingDecision {
            selected_server_id: selected.id,
            reason: format!(
                "Round-robin selected '{}' (index={}, health={:?})",
                selected.name, index, selected.health
            ),
            timestamp: Utc::now(),
        };

        self.storage.record_routing_decision(decision.clone()).await?;
        Ok(decision)
    }
}

fn is_open_circuit(server: &ServerEndpoint) -> bool {
    server.metadata.get(FAILURES_KEY).and_then(|v| v.as_u64()).unwrap_or(0)
        >= CIRCUIT_BREAKER_THRESHOLD
}

fn route_score(server: &ServerEndpoint) -> f64 {
    let latency_ms =
        server.metadata.get("last_latency_ms").and_then(|v| v.as_f64()).unwrap_or(500.0);
    let error_rate = server.metadata.get("error_rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let failures = server.metadata.get(FAILURES_KEY).and_then(|v| v.as_u64()).unwrap_or(0) as f64;

    let health_penalty = match server.health {
        HealthState::Healthy => 0.0,
        HealthState::Degraded => 40.0,
        HealthState::Unknown => 80.0,
        HealthState::Unhealthy => 120.0,
    };

    f64::from(server.weight)
        - (latency_ms / 20.0)
        - (error_rate * 100.0)
        - (failures * 15.0)
        - health_penalty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ServerEndpoint;
    use crate::storage::InMemoryStorage;
    use uuid::Uuid;

    fn endpoint(id: u128, weight: u32, health: HealthState) -> ServerEndpoint {
        ServerEndpoint {
            id: Uuid::from_u128(id),
            name: format!("srv-{id}"),
            kind: ProviderKind::Llm,
            endpoint_url: "http://127.0.0.1:1".to_string(),
            api_key: None,
            health,
            weight,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn engine() -> RoutingEngine {
        RoutingEngine::new(Arc::new(InMemoryStorage::new()))
    }

    #[tokio::test]
    async fn score_prefers_higher_weight() {
        let e = engine();
        let servers =
            vec![endpoint(1, 10, HealthState::Healthy), endpoint(2, 100, HealthState::Healthy)];
        let d = e.select(&servers, RoutingStrategy::Score).await.unwrap();
        assert_eq!(d.selected_server_id, Uuid::from_u128(2));
    }

    #[tokio::test]
    async fn score_skips_penalized_degraded() {
        let e = engine();
        let servers =
            vec![endpoint(1, 10, HealthState::Degraded), endpoint(2, 100, HealthState::Healthy)];
        let d = e.select(&servers, RoutingStrategy::Score).await.unwrap();
        assert_eq!(d.selected_server_id, Uuid::from_u128(2));
    }

    #[tokio::test]
    async fn round_robin_alternates() {
        let e = engine();
        let servers =
            vec![endpoint(1, 1, HealthState::Healthy), endpoint(2, 1, HealthState::Healthy)];
        let d1 = e.select(&servers, RoutingStrategy::RoundRobin).await.unwrap();
        let d2 = e.select(&servers, RoutingStrategy::RoundRobin).await.unwrap();
        assert_ne!(d1.selected_server_id, d2.selected_server_id);
    }

    #[tokio::test]
    async fn weighted_random_returns_member() {
        let e = engine();
        let servers =
            vec![endpoint(1, 50, HealthState::Healthy), endpoint(2, 50, HealthState::Healthy)];
        let d = e.select(&servers, RoutingStrategy::WeightedRandom).await.unwrap();
        assert!(servers.iter().any(|s| s.id == d.selected_server_id));
    }

    #[test]
    fn open_circuit_blocks_high_failures() {
        let mut s = endpoint(1, 100, HealthState::Healthy);
        s.metadata = serde_json::json!({ FAILURES_KEY: 3 });
        assert!(is_open_circuit(&s));

        let mut s2 = endpoint(2, 100, HealthState::Healthy);
        s2.metadata = serde_json::json!({ FAILURES_KEY: 2 });
        assert!(!is_open_circuit(&s2));
    }

    #[test]
    fn strategy_parsing() {
        assert_eq!("score".parse::<RoutingStrategy>().unwrap(), RoutingStrategy::Score);
        assert_eq!(
            "weighted_random".parse::<RoutingStrategy>().unwrap(),
            RoutingStrategy::WeightedRandom
        );
        assert!("bogus".parse::<RoutingStrategy>().is_err());
    }
}
