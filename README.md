# rustforge-mcp

An MCP (Model Context Protocol) server for Rust development.

`rustforge-mcp` provides a set of tools that allow AI models to interact with Rust projects, manage files, and execute cargo commands directly.

## Features

- **File Management**: List files, read content, and write to files.
- **Cargo Integration**: Run `cargo check`, `cargo test`, `cargo clippy`, `cargo add`, and raw `cargo` commands.
- **Connectivity**: Simple `ping` tool to check server status.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable version)
- A client that supports the Model Context Protocol (e.g., Claude Desktop, Gemini CLI)

## Installation

To install `rustforge-mcp` globally:

```bash
cargo install --path .
```

Or run it directly using `cargo run`.

## Usage

### Connecting with MCP Clients

#### Claude Desktop

Add the following to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "rustforge": {
      "command": "path/to/rustforge-mcp"
    }
  }
}
```

#### Gemini CLI

`rustforge-mcp` can be used with Gemini CLI by configuring it as an MCP server.

## Tools Provided

- `ping`: Ping the server to check connectivity.
- `list_files`: List files in a directory.
- `read_file`: Read the content of a file.
- `write_file`: Write content to a file.
- `cargo`: Run a raw cargo command.
- `cargo_check`: Run cargo check to find compilation errors.
- `cargo_test`: Run cargo test to run unit and integration tests.
- `cargo_clippy`: Run cargo clippy for linting.
- `cargo_add`: Add a dependency to the project.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
