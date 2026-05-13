use anyhow::{anyhow, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: String,
    pub management_api_token: String,
    pub database_url: Option<String>,
    pub use_in_memory_storage: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
        let management_api_token = env::var("MANAGEMENT_API_TOKEN")
            .map_err(|_| anyhow!("MANAGEMENT_API_TOKEN must be set"))?;
        let database_url = env::var("DATABASE_URL").ok();
        let use_in_memory_storage = env::var("USE_INMEMORY_STORAGE")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

        if !use_in_memory_storage && database_url.is_none() {
            return Err(anyhow!(
                "DATABASE_URL must be set unless USE_INMEMORY_STORAGE=true"
            ));
        }

        Ok(Self {
            bind_addr,
            management_api_token,
            database_url,
            use_in_memory_storage,
        })
    }
}
