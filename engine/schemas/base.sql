-- Rove core engine combined database schema
-- Single source of truth for all SQLite logic

-- 1. Tasks System
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    input TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed')),
    provider_used TEXT,
    duration_ms INTEGER,
    created_at INTEGER NOT NULL,
    completed_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);

CREATE TABLE IF NOT EXISTS task_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    step_order INTEGER NOT NULL,
    step_type TEXT NOT NULL CHECK(step_type IN ('user_message', 'assistant_message', 'tool_call', 'tool_result')),
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_steps_task_id ON task_steps(task_id, step_order);

CREATE VIRTUAL TABLE IF NOT EXISTS task_steps_fts USING fts5(
    task_id UNINDEXED,
    step_type UNINDEXED,
    content,
    content='task_steps',
    content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS task_steps_ai AFTER INSERT ON task_steps
WHEN EXISTS (SELECT 1 FROM tasks WHERE id = new.task_id)
BEGIN
  INSERT INTO task_steps_fts(rowid, task_id, step_type, content)
  VALUES (new.id, new.task_id, new.step_type, new.content);
END;

CREATE TRIGGER IF NOT EXISTS task_steps_ad AFTER DELETE ON task_steps BEGIN
  INSERT INTO task_steps_fts(task_steps_fts, rowid, task_id, step_type, content)
  VALUES ('delete', old.id, old.task_id, old.step_type, old.content);
END;

CREATE TRIGGER IF NOT EXISTS task_steps_au AFTER UPDATE ON task_steps BEGIN
  INSERT INTO task_steps_fts(task_steps_fts, rowid, task_id, step_type, content)
  VALUES ('delete', old.id, old.task_id, old.step_type, old.content);
  INSERT INTO task_steps_fts(rowid, task_id, step_type, content)
  VALUES (new.id, new.task_id, new.step_type, new.content);
END;

-- 2. Plugins
CREATE TABLE IF NOT EXISTS plugins (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    version TEXT NOT NULL,
    wasm_path TEXT NOT NULL,
    wasm_hash TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_plugins_enabled ON plugins(enabled);

-- 3. Secrets Cache
CREATE TABLE IF NOT EXISTS secrets_cache (
    key TEXT PRIMARY KEY,
    encrypted_value BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_secrets_cache_expires_at ON secrets_cache(expires_at);

-- 4. Rate Limits
CREATE TABLE IF NOT EXISTS rate_limits (
    source TEXT NOT NULL,
    tier INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    PRIMARY KEY (source, tier, timestamp)
);

CREATE INDEX IF NOT EXISTS idx_rate_limits_timestamp ON rate_limits(timestamp);
CREATE INDEX IF NOT EXISTS idx_rate_limits_source_tier ON rate_limits(source, tier, timestamp DESC);

-- 5. Episodic Memory & Insights
CREATE TABLE IF NOT EXISTS episodic_memory (
    id                TEXT PRIMARY KEY,
    task_id           TEXT NOT NULL,
    summary           TEXT NOT NULL,
    entities          TEXT,
    topics            TEXT,
    importance        REAL NOT NULL DEFAULT 0.5,
    consolidated      INTEGER NOT NULL DEFAULT 0,
    consolidation_id  TEXT,
    tags              TEXT,
    team_id           TEXT,
    created_at        INTEGER NOT NULL,
    domain            TEXT NOT NULL DEFAULT 'general',
    sensitive         INTEGER NOT NULL DEFAULT 0,
    last_accessed     INTEGER,
    access_count      INTEGER NOT NULL DEFAULT 0,
    embedding         BLOB,
    embedding_model   TEXT,
    embedding_generated_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_episodic_memory_task ON episodic_memory(task_id);
CREATE INDEX IF NOT EXISTS idx_episodic_memory_consolidated ON episodic_memory(consolidated);
CREATE INDEX IF NOT EXISTS idx_episodic_memory_created ON episodic_memory(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_episodic_memory_importance ON episodic_memory(importance DESC);
CREATE INDEX IF NOT EXISTS idx_episodic_memory_team ON episodic_memory(team_id);
CREATE INDEX IF NOT EXISTS idx_episodic_memory_domain ON episodic_memory(domain);
CREATE INDEX IF NOT EXISTS idx_episodic_memory_sensitive ON episodic_memory(sensitive);
CREATE INDEX IF NOT EXISTS idx_episodic_memory_last_accessed ON episodic_memory(last_accessed);
CREATE INDEX IF NOT EXISTS idx_episodic_embedding_null ON episodic_memory(id) WHERE embedding IS NULL;

CREATE TABLE IF NOT EXISTS consolidation_insights (
    id          TEXT PRIMARY KEY,
    insight     TEXT NOT NULL,
    source_ids  TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    domain      TEXT
);

CREATE INDEX IF NOT EXISTS idx_consolidation_insights_created ON consolidation_insights(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_consolidation_insights_domain ON consolidation_insights(domain);

CREATE VIRTUAL TABLE IF NOT EXISTS episodic_fts USING fts5(
    summary,
    entities,
    topics,
    tags,
    domain,
    content=episodic_memory,
    content_rowid=rowid
);

CREATE TRIGGER IF NOT EXISTS episodic_fts_ai AFTER INSERT ON episodic_memory BEGIN
    INSERT INTO episodic_fts(rowid, summary, entities, topics, tags, domain)
    VALUES (new.rowid, new.summary, new.entities, new.topics, new.tags, new.domain);
END;

CREATE TRIGGER IF NOT EXISTS episodic_fts_ad AFTER DELETE ON episodic_memory BEGIN
    INSERT INTO episodic_fts(episodic_fts, rowid, summary, entities, topics, tags, domain)
    VALUES ('delete', old.rowid, old.summary, old.entities, old.topics, old.tags, old.domain);
END;

CREATE TRIGGER IF NOT EXISTS episodic_fts_au AFTER UPDATE ON episodic_memory BEGIN
    INSERT INTO episodic_fts(episodic_fts, rowid, summary, entities, topics, tags, domain)
    VALUES ('delete', old.rowid, old.summary, old.entities, old.topics, old.tags, old.domain);
    INSERT INTO episodic_fts(rowid, summary, entities, topics, tags, domain)
    VALUES (new.rowid, new.summary, new.entities, new.topics, new.tags, new.domain);
END;

CREATE VIRTUAL TABLE IF NOT EXISTS insights_fts USING fts5(
    insight,
    domain,
    content=consolidation_insights,
    content_rowid=rowid
);

CREATE TRIGGER IF NOT EXISTS insights_fts_ai AFTER INSERT ON consolidation_insights BEGIN
    INSERT INTO insights_fts(rowid, insight, domain)
    VALUES (new.rowid, new.insight, new.domain);
END;

CREATE TRIGGER IF NOT EXISTS insights_fts_ad AFTER DELETE ON consolidation_insights BEGIN
    INSERT INTO insights_fts(insights_fts, rowid, insight, domain)
    VALUES ('delete', old.rowid, old.insight, old.domain);
END;

CREATE TRIGGER IF NOT EXISTS insights_fts_au AFTER UPDATE ON consolidation_insights BEGIN
    INSERT INTO insights_fts(insights_fts, rowid, insight, domain)
    VALUES ('delete', old.rowid, old.insight, old.domain);
    INSERT INTO insights_fts(rowid, insight, domain)
    VALUES (new.rowid, new.insight, new.domain);
END;

-- 6. Agent Actions / Audit Log
CREATE TABLE IF NOT EXISTS agent_actions (
    id              TEXT PRIMARY KEY,
    task_id         TEXT NOT NULL,
    action_type     TEXT NOT NULL,
    tool_name       TEXT NOT NULL,
    args_hash       TEXT NOT NULL,
    risk_tier       INTEGER NOT NULL,
    approved_by     TEXT,
    result_summary  TEXT,
    timestamp       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_actions_task ON agent_actions(task_id);
CREATE INDEX IF NOT EXISTS idx_agent_actions_timestamp ON agent_actions(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_agent_actions_tool ON agent_actions(tool_name);

-- 7. Pending Tasks & Gateway
CREATE TABLE IF NOT EXISTS pending_tasks (
    id         TEXT    PRIMARY KEY,
    input      TEXT    NOT NULL,
    source     TEXT    NOT NULL,
    status     TEXT    NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    done_at    INTEGER,
    error      TEXT,
    session_id TEXT,
    workspace  TEXT,
    team_id    TEXT,
    domain     TEXT    NOT NULL DEFAULT 'general',
    complexity TEXT    NOT NULL DEFAULT 'simple',
    sensitive  INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_pending_tasks_status ON pending_tasks(status);
CREATE INDEX IF NOT EXISTS idx_pending_tasks_session ON pending_tasks(session_id);
CREATE INDEX IF NOT EXISTS idx_pending_tasks_domain ON pending_tasks(domain);

CREATE VIEW IF NOT EXISTS pending_tasks_recovery AS
    SELECT * FROM pending_tasks
    WHERE status = 'running';

-- 8. Knowledge Graph
CREATE TABLE IF NOT EXISTS graph_nodes (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    type TEXT NOT NULL,
    properties TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_updated INTEGER NOT NULL,
    access_count INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_graph_nodes_type ON graph_nodes(type);
CREATE INDEX IF NOT EXISTS idx_graph_nodes_label ON graph_nodes(label);
CREATE INDEX IF NOT EXISTS idx_graph_nodes_created ON graph_nodes(created_at);

CREATE TABLE IF NOT EXISTS graph_edges (
    id TEXT PRIMARY KEY,
    from_id TEXT NOT NULL,
    to_id TEXT NOT NULL,
    relation TEXT NOT NULL,
    weight REAL DEFAULT 1.0,
    properties TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (from_id) REFERENCES graph_nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (to_id) REFERENCES graph_nodes(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_graph_edges_from ON graph_edges(from_id);
CREATE INDEX IF NOT EXISTS idx_graph_edges_to ON graph_edges(to_id);
CREATE INDEX IF NOT EXISTS idx_graph_edges_relation ON graph_edges(relation);
CREATE INDEX IF NOT EXISTS idx_graph_edges_weight ON graph_edges(weight);

CREATE TABLE IF NOT EXISTS memory_graph_links (
    memory_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    relevance REAL DEFAULT 1.0,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (memory_id, node_id),
    FOREIGN KEY (memory_id) REFERENCES episodic_memory(id) ON DELETE CASCADE,
    FOREIGN KEY (node_id) REFERENCES graph_nodes(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_memory_graph_memory ON memory_graph_links(memory_id);
CREATE INDEX IF NOT EXISTS idx_memory_graph_node ON memory_graph_links(node_id);

CREATE TABLE IF NOT EXISTS graph_extraction_queue (
    memory_id TEXT PRIMARY KEY,
    priority INTEGER DEFAULT 0,
    attempts INTEGER DEFAULT 0,
    last_attempt INTEGER,
    error TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (memory_id) REFERENCES episodic_memory(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_extraction_queue_priority ON graph_extraction_queue(priority DESC, created_at);
CREATE INDEX IF NOT EXISTS idx_extraction_queue_attempts ON graph_extraction_queue(attempts);

-- 9. Embeddings
CREATE TABLE IF NOT EXISTS embedding_queue (
    memory_id TEXT PRIMARY KEY,
    priority INTEGER DEFAULT 0,
    attempts INTEGER DEFAULT 0,
    last_attempt INTEGER,
    error TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (memory_id) REFERENCES episodic_memory(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_embedding_queue_priority ON embedding_queue(priority DESC, created_at);
CREATE INDEX IF NOT EXISTS idx_embedding_queue_attempts ON embedding_queue(attempts);

CREATE TABLE IF NOT EXISTS embedding_metadata (
    model_name TEXT PRIMARY KEY,
    dimensions INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    last_used INTEGER NOT NULL,
    total_embeddings INTEGER DEFAULT 0
);

INSERT OR IGNORE INTO embedding_metadata (model_name, dimensions, created_at, last_used)
VALUES 
    ('local-brain', 384, strftime('%s', 'now'), strftime('%s', 'now')),
    ('text-embedding-3-small', 1536, strftime('%s', 'now'), strftime('%s', 'now'));

-- 10. Agent Events
CREATE TABLE IF NOT EXISTS agent_events (
    id          TEXT    PRIMARY KEY,
    task_id     TEXT    NOT NULL,
    event_type  TEXT    NOT NULL,
    payload     TEXT    NOT NULL,
    step_num    INTEGER NOT NULL,
    domain      TEXT,
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_events_task ON agent_events(task_id, step_num);
CREATE INDEX IF NOT EXISTS idx_agent_events_domain ON agent_events(domain);
CREATE INDEX IF NOT EXISTS idx_agent_events_created ON agent_events(created_at DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS agent_events_fts USING fts5(
    task_id,
    event_type,
    payload,
    content=agent_events,
    content_rowid=rowid
);

CREATE TRIGGER IF NOT EXISTS agent_events_fts_insert AFTER INSERT ON agent_events BEGIN
    INSERT INTO agent_events_fts(rowid, task_id, event_type, payload)
    VALUES (new.rowid, new.task_id, new.event_type, new.payload);
END;

CREATE TRIGGER IF NOT EXISTS agent_events_fts_delete AFTER DELETE ON agent_events BEGIN
    INSERT INTO agent_events_fts(agent_events_fts, rowid, task_id, event_type, payload)
    VALUES ('delete', old.rowid, old.task_id, old.event_type, old.payload);
END;
