-- Rove core engine combined database schema
-- Single source of truth for all SQLite logic

-- 1. Tasks System
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    input TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'cli',
    agent_id TEXT,
    agent_name TEXT,
    thread_id TEXT,
    worker_preset_id TEXT,
    worker_preset_name TEXT,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed')),
    provider_used TEXT,
    duration_ms INTEGER,
    created_at INTEGER NOT NULL,
    completed_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_agent_created_at ON tasks(agent_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_thread_created_at ON tasks(thread_id, created_at DESC);

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

CREATE TABLE IF NOT EXISTS installed_plugins (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    version      TEXT NOT NULL,
    plugin_type  TEXT NOT NULL,
    trust_tier   INTEGER NOT NULL DEFAULT 2,
    manifest     TEXT NOT NULL,
    binary_path  TEXT,
    binary_hash  TEXT NOT NULL,
    signature    TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    installed_at INTEGER NOT NULL,
    last_used    INTEGER,
    config       TEXT,
    provenance_source TEXT,
    provenance_registry TEXT,
    catalog_trust_badge TEXT
);

CREATE INDEX IF NOT EXISTS idx_installed_plugins_type
  ON installed_plugins(plugin_type, enabled);

CREATE TABLE IF NOT EXISTS extension_catalog_entries (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    description TEXT NOT NULL,
    trust_badge TEXT NOT NULL,
    latest_version TEXT NOT NULL,
    latest_published_at INTEGER NOT NULL,
    registry_source TEXT NOT NULL,
    index_path TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    permission_summary_json TEXT NOT NULL DEFAULT '[]',
    permission_warnings_json TEXT NOT NULL DEFAULT '[]',
    release_summary TEXT,
    fetched_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_extension_catalog_kind
  ON extension_catalog_entries(kind, trust_badge, name);

CREATE INDEX IF NOT EXISTS idx_extension_catalog_fetched
  ON extension_catalog_entries(fetched_at DESC);

CREATE TABLE IF NOT EXISTS remote_discovery_candidates (
    candidate_id TEXT PRIMARY KEY,
    transport_kind TEXT NOT NULL,
    network_id TEXT,
    member_id TEXT NOT NULL,
    member_name TEXT,
    node_name_hint TEXT,
    node_identity_json TEXT NOT NULL DEFAULT 'null',
    node_profile_json TEXT NOT NULL DEFAULT 'null',
    assigned_addresses_json TEXT NOT NULL DEFAULT '[]',
    transports_json TEXT NOT NULL DEFAULT '[]',
    last_seen_at INTEGER NOT NULL,
    controller_access INTEGER NOT NULL DEFAULT 0,
    paired_node_name TEXT,
    trusted INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_remote_discovery_transport
  ON remote_discovery_candidates(transport_kind, network_id, last_seen_at DESC);

CREATE INDEX IF NOT EXISTS idx_remote_discovery_node_name
  ON remote_discovery_candidates(node_name_hint, member_name);

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
    memory_kind       TEXT NOT NULL DEFAULT 'general',
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
    memory_kind,
    content=episodic_memory,
    content_rowid=rowid
);

CREATE TRIGGER IF NOT EXISTS episodic_fts_ai AFTER INSERT ON episodic_memory BEGIN
    INSERT INTO episodic_fts(rowid, summary, entities, topics, tags, domain, memory_kind)
    VALUES (new.rowid, new.summary, new.entities, new.topics, new.tags, new.domain, new.memory_kind);
END;

CREATE TRIGGER IF NOT EXISTS episodic_fts_ad AFTER DELETE ON episodic_memory BEGIN
    INSERT INTO episodic_fts(episodic_fts, rowid, summary, entities, topics, tags, domain, memory_kind)
    VALUES ('delete', old.rowid, old.summary, old.entities, old.topics, old.tags, old.domain, old.memory_kind);
END;

CREATE TRIGGER IF NOT EXISTS episodic_fts_au AFTER UPDATE ON episodic_memory BEGIN
    INSERT INTO episodic_fts(episodic_fts, rowid, summary, entities, topics, tags, domain, memory_kind)
    VALUES ('delete', old.rowid, old.summary, old.entities, old.topics, old.tags, old.domain, old.memory_kind);
    INSERT INTO episodic_fts(rowid, summary, entities, topics, tags, domain, memory_kind)
    VALUES (new.rowid, new.summary, new.entities, new.topics, new.tags, new.domain, new.memory_kind);
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
    execution_profile_json TEXT,
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

-- 7.1 Scheduled Tasks
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    input         TEXT NOT NULL,
    target_kind   TEXT NOT NULL DEFAULT 'task',
    target_id     TEXT,
    interval_secs INTEGER NOT NULL CHECK(interval_secs > 0),
    enabled       INTEGER NOT NULL DEFAULT 1,
    workspace     TEXT,
    created_at    INTEGER NOT NULL,
    last_run_at   INTEGER,
    next_run_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_next_run
    ON scheduled_tasks(enabled, next_run_at);

-- 7.2 Agent And Workflow Runs
CREATE TABLE IF NOT EXISTS agent_runs (
    run_id           TEXT PRIMARY KEY,
    agent_id         TEXT NOT NULL,
    task_id          TEXT,
    workflow_run_id  TEXT,
    status           TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed')),
    input            TEXT NOT NULL,
    output           TEXT,
    error            TEXT,
    created_at       INTEGER NOT NULL,
    completed_at     INTEGER
);

CREATE INDEX IF NOT EXISTS idx_agent_runs_agent
    ON agent_runs(agent_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_runs_workflow
    ON agent_runs(workflow_run_id, created_at DESC);

CREATE TABLE IF NOT EXISTS workflow_runs (
    run_id         TEXT PRIMARY KEY,
    workflow_id    TEXT NOT NULL,
    status         TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
    input          TEXT NOT NULL,
    output         TEXT,
    error          TEXT,
    steps_total    INTEGER NOT NULL DEFAULT 0,
    steps_completed INTEGER NOT NULL DEFAULT 0,
    current_step_index INTEGER,
    current_step_id TEXT,
    current_step_name TEXT,
    retry_count    INTEGER NOT NULL DEFAULT 0,
    last_task_id   TEXT,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    cancel_requested_at INTEGER,
    created_at     INTEGER NOT NULL,
    completed_at   INTEGER
);

CREATE INDEX IF NOT EXISTS idx_workflow_runs_workflow
    ON workflow_runs(workflow_id, created_at DESC);

CREATE TABLE IF NOT EXISTS workflow_run_steps (
    run_id         TEXT NOT NULL,
    step_index     INTEGER NOT NULL,
    step_id        TEXT NOT NULL,
    step_name      TEXT NOT NULL,
    agent_id       TEXT,
    worker_preset  TEXT,
    status         TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed')),
    prompt         TEXT NOT NULL,
    task_id        TEXT,
    output         TEXT,
    error          TEXT,
    attempt_count  INTEGER NOT NULL DEFAULT 0,
    started_at     INTEGER NOT NULL,
    completed_at   INTEGER,
    PRIMARY KEY (run_id, step_index),
    FOREIGN KEY (run_id) REFERENCES workflow_runs(run_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_workflow_run_steps_run
    ON workflow_run_steps(run_id, step_index);

CREATE INDEX IF NOT EXISTS idx_workflow_run_steps_status
    ON workflow_run_steps(status, started_at DESC);

-- 8. Knowledge Graph
CREATE TABLE IF NOT EXISTS graph_nodes (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    type TEXT NOT NULL,
    properties TEXT NOT NULL,
    source_kind TEXT NOT NULL DEFAULT 'deterministic',
    source_scope TEXT NOT NULL DEFAULT 'per_node',
    source_ref TEXT,
    confidence REAL NOT NULL DEFAULT 1.0,
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
    source_kind TEXT NOT NULL DEFAULT 'deterministic',
    source_scope TEXT NOT NULL DEFAULT 'per_node',
    source_ref TEXT,
    confidence REAL NOT NULL DEFAULT 1.0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT 0,
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

CREATE TABLE IF NOT EXISTS memory_graph_sources (
    source_id         TEXT PRIMARY KEY,
    source_kind       TEXT NOT NULL,
    source_scope      TEXT NOT NULL DEFAULT 'per_node',
    workspace_path    TEXT,
    repo_name         TEXT,
    db_path           TEXT,
    source_last_updated TEXT,
    source_branch     TEXT,
    source_commit     TEXT,
    last_imported_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_graph_sources_kind
    ON memory_graph_sources(source_kind, repo_name);

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
    parent_task_id TEXT,
    event_type  TEXT    NOT NULL,
    payload     TEXT    NOT NULL,
    step_num    INTEGER NOT NULL,
    domain      TEXT,
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_events_task ON agent_events(task_id, step_num);
CREATE INDEX IF NOT EXISTS idx_agent_events_parent ON agent_events(parent_task_id, step_num);
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

-- 11. Local Daemon Auth / WebUI Sessions
CREATE TABLE IF NOT EXISTS auth_sessions (
    session_id           TEXT PRIMARY KEY,
    created_at           INTEGER NOT NULL,
    last_seen_at         INTEGER NOT NULL,
    expires_at           INTEGER NOT NULL,
    absolute_expires_at  INTEGER NOT NULL,
    revoked_at           INTEGER,
    client_label         TEXT,
    origin               TEXT,
    user_agent           TEXT,
    requires_reauth      INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_auth_sessions_expiry
    ON auth_sessions(expires_at, absolute_expires_at);
CREATE INDEX IF NOT EXISTS idx_auth_sessions_revoked
    ON auth_sessions(revoked_at);

CREATE TABLE IF NOT EXISTS auth_reauth (
    session_id   TEXT PRIMARY KEY,
    verified_at  INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES auth_sessions(session_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_auth_reauth_expiry
    ON auth_reauth(expires_at);

CREATE TABLE IF NOT EXISTS auth_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type  TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    session_id  TEXT,
    metadata    TEXT,
    FOREIGN KEY (session_id) REFERENCES auth_sessions(session_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_auth_events_created
    ON auth_events(created_at DESC);

CREATE TABLE IF NOT EXISTS auth_passkeys (
    id           TEXT PRIMARY KEY,
    user_uuid    TEXT NOT NULL,
    rp_id        TEXT NOT NULL,
    credential_id TEXT NOT NULL UNIQUE,
    label        TEXT,
    passkey_json TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_auth_passkeys_rp_id
    ON auth_passkeys(rp_id, created_at DESC);

CREATE TABLE IF NOT EXISTS auth_passkey_challenges (
    challenge_id TEXT PRIMARY KEY,
    challenge_type TEXT NOT NULL,
    session_id   TEXT,
    rp_id        TEXT NOT NULL,
    origin       TEXT NOT NULL,
    state_json   TEXT NOT NULL,
    label        TEXT,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES auth_sessions(session_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_auth_passkey_challenges_expiry
    ON auth_passkey_challenges(expires_at);

CREATE TABLE IF NOT EXISTS pending_approvals (
    approval_id  TEXT PRIMARY KEY,
    task_id      TEXT NOT NULL,
    risk_level   TEXT NOT NULL,
    summary      TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    resolved_at  INTEGER
);

CREATE INDEX IF NOT EXISTS idx_pending_approvals_created
    ON pending_approvals(created_at DESC);

CREATE TABLE IF NOT EXISTS telegram_audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type      TEXT NOT NULL,
    telegram_user   INTEGER NOT NULL,
    chat_id         INTEGER,
    task_id         TEXT,
    approval_key    TEXT,
    approved        INTEGER,
    operation       TEXT,
    created_at      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_telegram_audit_user
    ON telegram_audit_log(telegram_user, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_telegram_audit_task
    ON telegram_audit_log(task_id) WHERE task_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS knowledge_documents (
    id              TEXT PRIMARY KEY,
    source_type     TEXT NOT NULL,
    source_path     TEXT NOT NULL,
    title           TEXT,
    content         TEXT NOT NULL,
    content_hash    TEXT NOT NULL,
    mime_type       TEXT,
    size_bytes      INTEGER,
    word_count      INTEGER,
    domain          TEXT,
    tags            TEXT,
    indexed_at      INTEGER NOT NULL,
    last_accessed   INTEGER,
    access_count    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_knowledge_source
    ON knowledge_documents(source_type, source_path);

CREATE INDEX IF NOT EXISTS idx_knowledge_domain
    ON knowledge_documents(domain) WHERE domain IS NOT NULL;

CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts
    USING fts5(title, content, tags, source_path,
               content=knowledge_documents, content_rowid=rowid);

CREATE TRIGGER IF NOT EXISTS knowledge_fts_insert
    AFTER INSERT ON knowledge_documents BEGIN
        INSERT INTO knowledge_fts(rowid, title, content, tags, source_path)
        VALUES (new.rowid, new.title, new.content, new.tags, new.source_path);
    END;

CREATE TRIGGER IF NOT EXISTS knowledge_fts_delete
    AFTER DELETE ON knowledge_documents BEGIN
        INSERT INTO knowledge_fts(knowledge_fts, rowid, title, content, tags, source_path)
        VALUES ('delete', old.rowid, old.title, old.content, old.tags, old.source_path);
    END;

-- 12. Memory Facts — structured KV store (never decayed, always injected first)
-- Singleton facts use the key as PRIMARY KEY (upsert replaces old value).
-- Remembered-fact entries use "remembered_fact:<hash>" keys so each is unique.
CREATE TABLE IF NOT EXISTS memory_facts (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    task_id    TEXT,
    memory_id  TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_facts_updated ON memory_facts(updated_at DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_facts_fts USING fts5(
    key,
    value,
    content=memory_facts,
    content_rowid=rowid
);

CREATE TRIGGER IF NOT EXISTS memory_facts_ai AFTER INSERT ON memory_facts BEGIN
    INSERT INTO memory_facts_fts(rowid, key, value)
    VALUES (new.rowid, new.key, new.value);
END;

CREATE TRIGGER IF NOT EXISTS memory_facts_ad AFTER DELETE ON memory_facts BEGIN
    INSERT INTO memory_facts_fts(memory_facts_fts, rowid, key, value)
    VALUES ('delete', old.rowid, old.key, old.value);
END;

CREATE TRIGGER IF NOT EXISTS memory_facts_au AFTER UPDATE ON memory_facts BEGIN
    INSERT INTO memory_facts_fts(memory_facts_fts, rowid, key, value)
    VALUES ('delete', old.rowid, old.key, old.value);
    INSERT INTO memory_facts_fts(rowid, key, value)
    VALUES (new.rowid, new.key, new.value);
END;

-- 13. Memory Graph — memory-to-memory edges for BFS traversal
-- Connects episodic memories via shared entities, temporal proximity,
-- consolidation lineage (derived_from), and observed corroboration/contradiction.
-- Built deterministically at ingest time; no LLM required.
CREATE TABLE IF NOT EXISTS memory_graph_edges (
    id          TEXT PRIMARY KEY,
    from_id     TEXT NOT NULL,        -- episodic_memory.id (source)
    to_id       TEXT NOT NULL,        -- episodic_memory.id (target)
    edge_type   TEXT NOT NULL,        -- 'shares_entity' | 'temporal' | 'derived_from' | 'supports' | 'contradicts'
    entity      TEXT,                 -- entity name, set for shares_entity edges
    weight      REAL NOT NULL DEFAULT 1.0,
    confidence  REAL NOT NULL DEFAULT 1.0,
    source_kind TEXT NOT NULL DEFAULT 'deterministic',
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mem_graph_from       ON memory_graph_edges(from_id);
CREATE INDEX IF NOT EXISTS idx_mem_graph_to         ON memory_graph_edges(to_id);
CREATE INDEX IF NOT EXISTS idx_mem_graph_entity     ON memory_graph_edges(entity) WHERE entity IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_mem_graph_type_from  ON memory_graph_edges(edge_type, from_id);

-- 14. Memory versioning + audit
CREATE TABLE IF NOT EXISTS memory_versions (
    id              TEXT PRIMARY KEY,
    entity_kind     TEXT NOT NULL,
    entity_id       TEXT NOT NULL,
    version_num     INTEGER NOT NULL,
    action          TEXT NOT NULL,
    content_hash    TEXT NOT NULL,
    snapshot_json   TEXT NOT NULL,
    actor           TEXT NOT NULL,
    source_task_id  TEXT,
    created_at      INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_versions_entity_version
    ON memory_versions(entity_kind, entity_id, version_num);
CREATE INDEX IF NOT EXISTS idx_memory_versions_entity_created
    ON memory_versions(entity_kind, entity_id, created_at DESC);

CREATE TABLE IF NOT EXISTS memory_audit_log (
    id                 TEXT PRIMARY KEY,
    entity_kind        TEXT NOT NULL,
    entity_id          TEXT NOT NULL,
    action             TEXT NOT NULL,
    actor              TEXT NOT NULL,
    source_task_id     TEXT,
    precondition_hash  TEXT,
    content_hash       TEXT,
    metadata_json      TEXT,
    created_at         INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_audit_entity_created
    ON memory_audit_log(entity_kind, entity_id, created_at DESC);
