use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router, ServiceExt};
use serde::Deserialize;
use schemars::JsonSchema;
use tokio::fs;
use tokio::process::Command;

#[derive(Clone, Default)]
struct RustForgeServer;

#[derive(Deserialize, JsonSchema)]
struct PingParams {
    #[schemars(description = "An optional message to echo back")]
    message: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ListFilesParams {
    #[schemars(description = "The path to list files in")]
    path: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ReadFileParams {
    #[schemars(description = "The path of the file to read")]
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct WriteFileParams {
    #[schemars(description = "The path of the file to write")]
    path: String,
    #[schemars(description = "The content to write to the file")]
    content: String,
}

#[derive(Deserialize, JsonSchema, Default)]
struct CargoParams {
    #[schemars(description = "Arguments to pass to cargo")]
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct CargoAddParams {
    #[schemars(description = "The package to add")]
    package: String,
}

#[tool_router(server_handler)]
impl RustForgeServer {
    #[tool(description = "Ping the server to check connectivity")]
    fn ping(&self, Parameters(PingParams { message }): Parameters<PingParams>) -> String {
        format!("Pong! {}", message.unwrap_or_else(|| "No message".to_string()))
    }

    #[tool(description = "List files in a directory")]
    async fn list_files(&self, Parameters(args): Parameters<ListFilesParams>) -> String {
        let path = args.path.unwrap_or_else(|| ".".to_string());
        match fs::read_dir(path).await {
            Ok(mut entries) => {
                let mut files = Vec::new();
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let is_dir = path.is_dir();
                    files.push(format!("{}{}", name, if is_dir { "/" } else { "" }));
                }
                files.join("\n")
            }
            Err(e) => e.to_string(),
        }
    }

    #[tool(description = "Read the content of a file")]
    async fn read_file(&self, Parameters(args): Parameters<ReadFileParams>) -> String {
        match fs::read_to_string(args.path).await {
            Ok(content) => content,
            Err(e) => e.to_string(),
        }
    }

    #[tool(description = "Write content to a file")]
    async fn write_file(&self, Parameters(args): Parameters<WriteFileParams>) -> String {
        match fs::write(args.path, args.content).await {
            Ok(_) => "File written successfully".to_string(),
            Err(e) => e.to_string(),
        }
    }

    #[tool(description = "Run a raw cargo command")]
    async fn cargo(&self, Parameters(args): Parameters<CargoParams>) -> String {
        match Command::new("cargo")
            .args(&args.args)
            .output()
            .await
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                format!("STDOUT:\n{}\n\nSTDERR:\n{}", stdout, stderr)
            }
            Err(e) => e.to_string(),
        }
    }

    #[tool(description = "Run cargo check to find compilation errors")]
    async fn cargo_check(&self) -> String {
        self.cargo(Parameters(CargoParams { args: vec!["check".to_string()] })).await
    }

    #[tool(description = "Run cargo test to run unit and integration tests")]
    async fn cargo_test(&self, Parameters(args): Parameters<CargoParams>) -> String {
        let mut full_args = vec!["test".to_string()];
        full_args.extend(args.args);
        self.cargo(Parameters(CargoParams { args: full_args })).await
    }

    #[tool(description = "Run cargo clippy for linting")]
    async fn cargo_clippy(&self) -> String {
        self.cargo(Parameters(CargoParams { args: vec!["clippy".to_string()] })).await
    }

    #[tool(description = "Add a dependency to the project")]
    async fn cargo_add(&self, Parameters(args): Parameters<CargoAddParams>) -> String {
        self.cargo(Parameters(CargoParams { args: vec!["add".to_string(), args.package] })).await
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Start the server using stdio transport
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let service = RustForgeServer.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
