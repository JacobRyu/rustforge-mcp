-- Initial schema for Multiple MCP/LLM Switching Service

CREATE TABLE IF NOT EXISTS servers (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    endpoint_url TEXT NOT NULL,
    api_key TEXT,
    health TEXT NOT NULL,
    weight INTEGER NOT NULL,
    metadata JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID PRIMARY KEY,
    action TEXT NOT NULL,
    user_id TEXT,
    details TEXT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS routing_history (
    id UUID PRIMARY KEY,
    selected_server_id UUID NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    reason TEXT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL
);
