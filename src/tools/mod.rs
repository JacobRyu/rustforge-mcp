use crate::config::SafetyConfig;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[derive(Clone)]
pub struct RustForgeServer {
    config: Arc<SafetyConfig>,
}

impl RustForgeServer {
    pub fn new(config: SafetyConfig) -> Self {
        Self { config: Arc::new(config) }
    }
}

impl Default for RustForgeServer {
    fn default() -> Self {
        Self::new(SafetyConfig::default())
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct PingParams {
    #[schemars(description = "An optional message to echo back")]
    pub message: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListFilesParams {
    #[schemars(description = "The path to list files in (relative to the workspace root)")]
    pub path: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadFileParams {
    #[schemars(description = "The path of the file to read (relative to the workspace root)")]
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct WriteFileParams {
    #[schemars(description = "The path of the file to write (relative to the workspace root)")]
    pub path: String,
    #[schemars(description = "The content to write to the file")]
    pub content: String,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct CargoParams {
    #[schemars(description = "Arguments to pass to cargo (subcommand must be allowed)")]
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CargoAddParams {
    #[schemars(description = "The package to add")]
    pub package: String,
}

#[tool_router(server_handler)]
impl RustForgeServer {
    #[tool(description = "Ping the server to check connectivity")]
    pub fn ping(&self, Parameters(PingParams { message }): Parameters<PingParams>) -> String {
        format!("Pong! {}", message.unwrap_or_else(|| "No message".to_string()))
    }

    #[tool(description = "List files in a directory within the workspace")]
    pub async fn list_files(&self, Parameters(args): Parameters<ListFilesParams>) -> String {
        let path = args.path.unwrap_or_else(|| ".".to_string());
        match self.resolve_path(&path) {
            Ok(dir) => match fs::read_dir(&dir).await {
                Ok(mut entries) => {
                    let mut files = Vec::new();
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let path = entry.path();
                        let name =
                            path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let is_dir = path.is_dir();
                        files.push(format!("{}{}", name, if is_dir { "/" } else { "" }));
                    }
                    files.join("\n")
                }
                Err(e) => e.to_string(),
            },
            Err(e) => e.to_string(),
        }
    }

    #[tool(description = "Read the content of a file within the workspace")]
    pub async fn read_file(&self, Parameters(args): Parameters<ReadFileParams>) -> String {
        match self.resolve_path(&args.path) {
            Ok(path) => match fs::read_to_string(path).await {
                Ok(content) => content,
                Err(e) => e.to_string(),
            },
            Err(e) => e.to_string(),
        }
    }

    #[tool(description = "Write content to a file within the workspace")]
    pub async fn write_file(&self, Parameters(args): Parameters<WriteFileParams>) -> String {
        match self.resolve_path(&args.path) {
            Ok(path) => match fs::write(path, args.content).await {
                Ok(_) => "File written successfully".to_string(),
                Err(e) => e.to_string(),
            },
            Err(e) => e.to_string(),
        }
    }

    #[tool(description = "Run a raw cargo command (subcommand must be on the allowlist)")]
    pub async fn cargo(&self, Parameters(args): Parameters<CargoParams>) -> String {
        if let Err(e) = validate_cargo_args(&args.args, &self.config.allowed_cargo_commands) {
            return format!("cargo command rejected: {e}");
        }

        let root = self.config.workspace_root.clone();
        let timeout = self.config.cargo_timeout;
        let mut child = match Command::new("cargo")
            .args(&args.args)
            .current_dir(&root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => return format!("Failed to spawn cargo: {e}"),
        };

        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();

        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(_)) => {
                let mut out = String::new();
                let mut err = String::new();
                if let Some(s) = &mut stdout {
                    let _ = s.read_to_string(&mut out).await;
                }
                if let Some(s) = &mut stderr {
                    let _ = s.read_to_string(&mut err).await;
                }
                format!("STDOUT:\n{out}\n\nSTDERR:\n{err}")
            }
            Ok(Err(e)) => e.to_string(),
            Err(_) => {
                let _ = child.kill().await;
                format!("cargo command timed out after {timeout:?}")
            }
        }
    }

    #[tool(description = "Run cargo check to find compilation errors")]
    pub async fn cargo_check(&self) -> String {
        self.cargo(Parameters(CargoParams { args: vec!["check".to_string()] })).await
    }

    #[tool(description = "Run cargo test to run unit and integration tests")]
    pub async fn cargo_test(&self, Parameters(args): Parameters<CargoParams>) -> String {
        let mut full_args = vec!["test".to_string()];
        full_args.extend(args.args);
        self.cargo(Parameters(CargoParams { args: full_args })).await
    }

    #[tool(description = "Run cargo clippy for linting")]
    pub async fn cargo_clippy(&self) -> String {
        self.cargo(Parameters(CargoParams { args: vec!["clippy".to_string()] })).await
    }

    #[tool(description = "Add a dependency to the project")]
    pub async fn cargo_add(&self, Parameters(args): Parameters<CargoAddParams>) -> String {
        self.cargo(Parameters(CargoParams { args: vec!["add".to_string(), args.package] })).await
    }

    /// ワークスペースルートにパスを制限する
    fn resolve_path(&self, path: &str) -> anyhow::Result<PathBuf> {
        let root = self
            .config
            .workspace_root
            .canonicalize()
            .unwrap_or_else(|_| self.config.workspace_root.clone());
        let raw = Path::new(path);
        let candidate = if raw.is_absolute() { raw.to_path_buf() } else { root.join(raw) };
        let candidate = normalize_path(&candidate);
        if candidate.starts_with(&root) {
            Ok(candidate.canonicalize().unwrap_or(candidate))
        } else {
            anyhow::bail!("path escapes the workspace root: {path}")
        }
    }
}

/// `..` や `.` を周期的に解決する（ファイルシステムに触れない正規化）
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !path.is_absolute() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// cargo の引数を許可リストとシェルメタ文字で検証する
fn validate_cargo_args(args: &[String], allowed: &[String]) -> anyhow::Result<()> {
    let subcommand = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("no cargo subcommand provided"))?;

    if !allowed.iter().any(|a| a == subcommand) {
        anyhow::bail!(
            "cargo subcommand '{subcommand}' is not allowed (allowed: {})",
            allowed.join(", ")
        );
    }

    for arg in args {
        if arg
            .chars()
            .any(|c| matches!(c, ';' | '|' | '&' | '`' | '$' | '(' | ')' | '<' | '>' | '\n' | '\r'))
        {
            anyhow::bail!("argument '{}' contains shell metacharacters", arg);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_allows_allowlisted_subcommand() {
        let allowed = vec!["build".to_string(), "check".to_string()];
        assert!(validate_cargo_args(&["check".into()], &allowed).is_ok());
        assert!(validate_cargo_args(&["--color".into(), "check".into()], &allowed).is_ok());
    }

    #[test]
    fn validate_rejects_unknown_subcommand() {
        let allowed = vec!["build".to_string()];
        assert!(validate_cargo_args(&["publish".into()], &allowed).is_err());
        assert!(validate_cargo_args(&["clean".into()], &allowed).is_err());
    }

    #[test]
    fn validate_rejects_shell_metacharacters() {
        let allowed = vec!["build".to_string()];
        assert!(
            validate_cargo_args(&["build".into(), "--cfg".into(), "x;rm".into()], &allowed)
                .is_err()
        );
        assert!(validate_cargo_args(&["build".into(), "src/$(pwd)".into()], &allowed).is_err());
        assert!(validate_cargo_args(&["check".into(), "a|b".into()], &allowed).is_err());
    }

    #[test]
    fn resolve_path_rejects_escape() {
        let root = std::env::temp_dir();
        let server = RustForgeServer::new(SafetyConfig {
            workspace_root: root,
            allowed_cargo_commands: vec!["check".into()],
            cargo_timeout: std::time::Duration::from_secs(5),
        });
        assert!(server.resolve_path("/etc/passwd").is_err());
        assert!(server.resolve_path("../../etc/passwd").is_err());
        assert!(server.resolve_path(".").is_ok());
    }
}
