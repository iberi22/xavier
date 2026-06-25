-- Enable pgvector extension
CREATE EXTENSION IF NOT EXISTS vector;

-- memory_records
CREATE TABLE IF NOT EXISTS memory_records (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    path TEXT,
    content TEXT NOT NULL,
    metadata JSONB DEFAULT '{}',
    embedding VECTOR(1536),
    tags TEXT[] DEFAULT '{}',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    parent_id TEXT REFERENCES memory_records(id),
    node_id TEXT DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_memory_workspace ON memory_records(workspace_id);
CREATE INDEX IF NOT EXISTS idx_memory_node ON memory_records(node_id);
CREATE INDEX IF NOT EXISTS idx_memory_parent ON memory_records(parent_id);

-- belief_states
CREATE TABLE IF NOT EXISTS belief_states (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    beliefs JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- session_tokens
CREATE TABLE IF NOT EXISTS session_tokens (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    token TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_session_workspace ON session_tokens(workspace_id);

-- checkpoint_records
CREATE TABLE IF NOT EXISTS checkpoint_records (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    name TEXT NOT NULL,
    data JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_checkpoint_workspace_task ON checkpoint_records(workspace_id, task_id);

-- Enable Row Level Security
ALTER TABLE memory_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE belief_states ENABLE ROW LEVEL SECURITY;
ALTER TABLE session_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE checkpoint_records ENABLE ROW LEVEL SECURITY;
