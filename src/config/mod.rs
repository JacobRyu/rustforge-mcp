use crate::router::RoutingStrategy;
use anyhow::{Result, anyhow};
use std::env;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: String,
    pub management_api_token: String,
    pub viewer_api_token: Option<String>,
    pub database_url: Option<String>,
    pub use_in_memory_storage: bool,
    pub routing_strategy: RoutingStrategy,
    pub safety: SafetyConfig,
}

/// コマンド実行・ファイル操作の安全設定
#[derive(Debug, Clone)]
pub struct SafetyConfig {
    pub workspace_root: PathBuf,
    pub allowed_cargo_commands: Vec<String>,
    pub cargo_timeout: Duration,
}

impl SafetyConfig {
    pub fn from_env() -> Result<Self> {
        let workspace_root =
            PathBuf::from(env::var("WORKSPACE_ROOT").unwrap_or_else(|_| ".".into()));
        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);

        let allowed_cargo_commands = env::var("CARGO_ALLOWED_COMMANDS")
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|_| {
                vec![
                    "build".into(),
                    "check".into(),
                    "test".into(),
                    "clippy".into(),
                    "fmt".into(),
                    "add".into(),
                    "doc".into(),
                ]
            });

        let cargo_timeout_secs =
            env::var("CARGO_TIMEOUT_SECS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(300);

        Ok(Self {
            workspace_root,
            allowed_cargo_commands,
            cargo_timeout: Duration::from_secs(cargo_timeout_secs.max(1)),
        })
    }
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            allowed_cargo_commands: vec![
                "build".into(),
                "check".into(),
                "test".into(),
                "clippy".into(),
                "fmt".into(),
                "add".into(),
                "doc".into(),
            ],
            cargo_timeout: Duration::from_secs(300),
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
        let management_api_token = env::var("MANAGEMENT_API_TOKEN")
            .map_err(|_| anyhow!("MANAGEMENT_API_TOKEN must be set"))?;
        let viewer_api_token = env::var("VIEWER_API_TOKEN").ok();
        let database_url = env::var("DATABASE_URL").ok();
        let use_in_memory_storage = env::var("USE_INMEMORY_STORAGE")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);
        let routing_strategy = env::var("ROUTING_STRATEGY")
            .unwrap_or_else(|_| "score".to_string())
            .parse::<RoutingStrategy>()?;
        let safety = SafetyConfig::from_env()?;

        if !use_in_memory_storage && database_url.is_none() {
            return Err(anyhow!("DATABASE_URL must be set unless USE_INMEMORY_STORAGE=true"));
        }

        Ok(Self {
            bind_addr,
            management_api_token,
            viewer_api_token,
            database_url,
            use_in_memory_storage,
            routing_strategy,
            safety,
        })
    }
}
