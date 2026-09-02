use crate::models::HealthState;
use crate::storage::Storage;
use chrono::Utc;
use reqwest::Client;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{Duration, sleep};

use once_cell::sync::Lazy;
use prometheus::{Counter, Encoder, Registry, TextEncoder, register_counter_with_registry};

pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);
pub static ROUTING_REQUESTS: Lazy<Counter> = Lazy::new(|| {
    register_counter_with_registry!(
        "routing_requests_total",
        "Total number of routing requests",
        *REGISTRY
    )
    .unwrap()
});
pub static HEALTH_CHECK_FAILURE: Lazy<Counter> = Lazy::new(|| {
    register_counter_with_registry!(
        "health_check_failures_total",
        "Total number of health check failures",
        *REGISTRY
    )
    .unwrap()
});

pub fn metrics_handler() -> String {
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

pub async fn health_check_worker(storage: Arc<dyn Storage>) {
    tracing::info!("Starting health check worker");
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .expect("failed to build health check client");

    loop {
        let servers = match storage.list_servers().await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to list servers for health check: {}", e);
                sleep(Duration::from_secs(30)).await;
                continue;
            }
        };

        for mut server in servers {
            let (new_health, latency_ms) = perform_health_check(&client, &server).await;
            let previous_health = server.health.clone();
            let mut should_write = previous_health != new_health;

            if let Some(metadata) = server.metadata.as_object_mut() {
                metadata.insert(
                    "last_checked_at".to_string(),
                    serde_json::json!(Utc::now().to_rfc3339()),
                );
                if let Some(latency) = latency_ms {
                    metadata.insert("last_latency_ms".to_string(), serde_json::json!(latency));
                }
                // サーキットブレーカー用の連続失敗カウンタ更新
                let failures =
                    metadata.get(crate::router::FAILURES_KEY).and_then(|v| v.as_u64()).unwrap_or(0);
                let next_failures = match new_health {
                    HealthState::Healthy => 0,
                    HealthState::Degraded | HealthState::Unhealthy => failures + 1,
                    HealthState::Unknown => failures,
                };
                if next_failures != failures {
                    metadata.insert(
                        crate::router::FAILURES_KEY.to_string(),
                        serde_json::json!(next_failures),
                    );
                }
                should_write = true;
            }

            if should_write {
                let server_name = server.name.clone();
                if previous_health != new_health {
                    tracing::info!(
                        "Server {} health changed: {:?} -> {:?}",
                        server_name,
                        previous_health,
                        new_health
                    );
                }
                server.health = new_health;
                server.updated_at = Utc::now();
                if let Err(e) = storage.upsert_server(server).await {
                    tracing::error!("Failed to update server health for {}: {}", server_name, e);
                }
            }
        }

        sleep(Duration::from_secs(60)).await;
    }
}

async fn perform_health_check(
    client: &Client,
    server: &crate::models::ServerEndpoint,
) -> (HealthState, Option<f64>) {
    tracing::debug!("Checking health for {} at {}", server.name, server.endpoint_url);

    if !server.endpoint_url.starts_with("http://") && !server.endpoint_url.starts_with("https://") {
        return (HealthState::Unknown, None);
    }

    let started = Instant::now();
    match client.get(&server.endpoint_url).send().await {
        Ok(response) if response.status().is_success() => {
            (HealthState::Healthy, Some(started.elapsed().as_secs_f64() * 1000.0))
        }
        Ok(_) => {
            HEALTH_CHECK_FAILURE.inc();
            (HealthState::Degraded, Some(started.elapsed().as_secs_f64() * 1000.0))
        }
        Err(_) => {
            HEALTH_CHECK_FAILURE.inc();
            (HealthState::Unhealthy, None)
        }
    }
}
