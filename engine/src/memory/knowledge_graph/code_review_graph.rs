use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{ConnectOptions, Row, SqlitePool};

use crate::conductor::types::GraphSourceKind;
use crate::memory::knowledge_graph::{
    EntityType, GraphEdge, GraphNode, KnowledgeGraph, RelationType,
};

const SOURCE_KIND: &str = "code_review_graph";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeReviewGraphRepoStatus {
    pub repo_name: String,
    pub repo_path: String,
    pub db_path: String,
    pub available: bool,
    pub imported: bool,
    pub stale: bool,
    pub nodes: usize,
    pub edges: usize,
    pub files: usize,
    pub last_updated: Option<String>,
    pub built_branch: Option<String>,
    pub built_commit: Option<String>,
    pub current_branch: Option<String>,
    pub current_commit: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeReviewGraphWorkspaceStatus {
    pub healthy: bool,
    pub available_count: usize,
    pub imported_count: usize,
    pub stale_count: usize,
    pub repos: Vec<CodeReviewGraphRepoStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeReviewGraphImportReport {
    pub imported_repos: usize,
    pub imported_nodes: usize,
    pub imported_edges: usize,
    pub skipped_repos: usize,
    pub repos: Vec<CodeReviewGraphRepoStatus>,
}

pub async fn workspace_status(
    pool: &SqlitePool,
    workspace_root: &Path,
) -> Result<CodeReviewGraphWorkspaceStatus> {
    let repos = discover_repositories(workspace_root)?;
    let mut statuses = Vec::new();
    let mut available_count = 0usize;
    let mut imported_count = 0usize;
    let mut stale_count = 0usize;

    for (name, path) in repos {
        let status = repo_status(pool, &name, &path).await?;
        if status.available {
            available_count += 1;
        }
        if status.imported {
            imported_count += 1;
        }
        if status.stale {
            stale_count += 1;
        }
        statuses.push(status);
    }

    Ok(CodeReviewGraphWorkspaceStatus {
        healthy: available_count > 0 && stale_count == 0,
        available_count,
        imported_count,
        stale_count,
        repos: statuses,
    })
}

pub async fn ensure_workspace_imported(
    pool: &SqlitePool,
    graph: &KnowledgeGraph,
    workspace_root: &Path,
    source_scope: &str,
) -> Result<CodeReviewGraphImportReport> {
    let repos = discover_repositories(workspace_root)?;
    let mut report = CodeReviewGraphImportReport::default();

    for (repo_name, repo_path) in repos {
        let status = repo_status(pool, &repo_name, &repo_path).await?;
        if !status.available {
            report.skipped_repos += 1;
            report.repos.push(status);
            continue;
        }

        if status.imported && !status.stale {
            report.skipped_repos += 1;
            report.repos.push(status);
            continue;
        }

        let imported = import_repo(pool, graph, &repo_name, &repo_path, source_scope).await?;
        report.imported_repos += 1;
        report.imported_nodes += imported.nodes;
        report.imported_edges += imported.edges;
        report.repos.push(imported);
    }

    Ok(report)
}

async fn repo_status(
    pool: &SqlitePool,
    repo_name: &str,
    repo_path: &Path,
) -> Result<CodeReviewGraphRepoStatus> {
    let db_path = repo_path.join(".code-review-graph").join("graph.db");
    let mut status = CodeReviewGraphRepoStatus {
        repo_name: repo_name.to_string(),
        repo_path: repo_path.display().to_string(),
        db_path: db_path.display().to_string(),
        ..Default::default()
    };

    if !db_path.exists() {
        status.message = Some("code-review-graph database missing".to_string());
        return Ok(status);
    }

    let db = open_graph_db(&db_path).await?;
    let nodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
        .fetch_one(&db)
        .await
        .context("Failed to count code-review-graph nodes")?;
    let edges: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM edges")
        .fetch_one(&db)
        .await
        .context("Failed to count code-review-graph edges")?;
    let files: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT file_path) FROM nodes")
        .fetch_one(&db)
        .await
        .context("Failed to count code-review-graph files")?;

    let metadata = load_metadata(&db).await?;
    let current = git_info(repo_path);
    let import_record = load_import_record(pool, repo_name).await?;

    status.available = true;
    status.nodes = nodes.max(0) as usize;
    status.edges = edges.max(0) as usize;
    status.files = files.max(0) as usize;
    status.last_updated = metadata.get("last_updated").cloned();
    status.built_branch = metadata.get("git_branch").cloned();
    status.built_commit = metadata.get("git_head_sha").cloned();
    status.current_branch = current.0;
    status.current_commit = current.1;
    status.imported = import_record.is_some();

    if let Some(record) = import_record {
        let same_commit = record.source_commit == status.built_commit;
        let same_updated = record.source_last_updated == status.last_updated;
        status.stale = !(same_commit && same_updated);
    } else {
        status.stale = true;
    }

    if let (Some(built_branch), Some(current_branch)) = (
        status.built_branch.as_deref(),
        status.current_branch.as_deref(),
    ) {
        if built_branch != current_branch {
            status.stale = true;
            status.message = Some(format!(
                "graph built on branch '{}' but repo is on '{}'",
                built_branch, current_branch
            ));
        }
    }

    Ok(status)
}

#[derive(Debug, Clone)]
struct ImportRecord {
    source_last_updated: Option<String>,
    source_commit: Option<String>,
}

async fn load_import_record(pool: &SqlitePool, repo_name: &str) -> Result<Option<ImportRecord>> {
    let row = sqlx::query(
        r#"SELECT source_last_updated, source_commit
           FROM memory_graph_sources
           WHERE source_id = ?"#,
    )
    .bind(format!("crg:{repo_name}"))
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| ImportRecord {
        source_last_updated: row.get("source_last_updated"),
        source_commit: row.get("source_commit"),
    }))
}

async fn load_metadata(pool: &SqlitePool) -> Result<HashMap<String, String>> {
    let rows = sqlx::query("SELECT key, value FROM metadata")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get::<String, _>("key"), row.get::<String, _>("value")))
        .collect())
}

async fn open_graph_db(path: &Path) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .read_only(true)
        .create_if_missing(false)
        .disable_statement_logging();
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .with_context(|| {
            format!(
                "Failed to open code-review-graph database at {}",
                path.display()
            )
        })
}

fn discover_repositories(workspace_root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut repos = Vec::new();
    let manifest = workspace_root.join("ops").join("repos.toml");
    if manifest.exists() {
        let raw = fs::read_to_string(&manifest)
            .with_context(|| format!("Failed to read {}", manifest.display()))?;
        let value: toml::Value = toml::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", manifest.display()))?;
        if let Some(repo_table) = value.get("repo").and_then(|entry| entry.as_table()) {
            for (name, repo_value) in repo_table {
                let Some(path_value) = repo_value.get("path").and_then(|entry| entry.as_str())
                else {
                    continue;
                };
                let repo_path = workspace_root.join(path_value);
                if repo_path.join(".git").exists() {
                    repos.push((name.clone(), repo_path));
                }
            }
        }
    }

    if repos.is_empty() {
        if workspace_root.join(".git").exists() {
            repos.push(("workspace".to_string(), workspace_root.to_path_buf()));
        }
        for entry in fs::read_dir(workspace_root)
            .with_context(|| format!("Failed to read {}", workspace_root.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path();
            if path.join(".git").exists() {
                repos.push((entry.file_name().to_string_lossy().to_string(), path));
            }
        }
    }

    repos.sort_by(|a, b| a.0.cmp(&b.0));
    repos.dedup_by(|a, b| a.1 == b.1);
    Ok(repos)
}

async fn import_repo(
    pool: &SqlitePool,
    graph: &KnowledgeGraph,
    repo_name: &str,
    repo_path: &Path,
    source_scope: &str,
) -> Result<CodeReviewGraphRepoStatus> {
    let db_path = repo_path.join(".code-review-graph").join("graph.db");
    let db = open_graph_db(&db_path).await?;
    let metadata = load_metadata(&db).await?;
    let communities = load_communities(&db).await?;
    let now = chrono::Utc::now().timestamp();

    delete_existing_import(pool, repo_name).await?;

    let rows = sqlx::query(
        r#"SELECT kind, name, qualified_name, file_path, line_start, line_end, language, is_test, signature, community_id
           FROM nodes"#,
    )
    .fetch_all(&db)
    .await
    .context("Failed to read code-review-graph nodes")?;

    let mut node_ids = HashMap::new();
    let mut seen_community_nodes = HashSet::new();
    for row in rows {
        let kind: String = row.get("kind");
        let qualified_name: String = row.get("qualified_name");
        let file_path: String = row.get("file_path");
        let name: String = row.get("name");
        let community_id: Option<i64> = row.get("community_id");
        let entity_type = map_crg_entity_type(&kind, row.get::<i64, _>("is_test") != 0);
        let label = normalized_label(repo_name, &kind, &name, &qualified_name, &file_path);
        let node_id = KnowledgeGraph::canonical_node_id(
            &entity_type,
            &format!("{repo_name}:{qualified_name}"),
        );
        let properties = serde_json::json!({
            "repo_name": repo_name,
            "repo_path": repo_path.display().to_string(),
            "code_review_graph": {
                "kind": kind,
                "qualified_name": qualified_name,
                "file_path": file_path,
                "line_start": row.get::<Option<i64>, _>("line_start"),
                "line_end": row.get::<Option<i64>, _>("line_end"),
                "language": row.get::<Option<String>, _>("language"),
                "signature": row.get::<Option<String>, _>("signature"),
                "community_id": community_id,
            }
        });

        graph
            .upsert_node(&GraphNode {
                id: node_id.clone(),
                label,
                node_type: entity_type,
                properties,
                source_kind: GraphSourceKind::CodeReviewGraph,
                source_scope: source_scope.to_string(),
                source_ref: Some(format!("{repo_name}:{qualified_name}")),
                confidence: 1.0,
                created_at: now,
                last_updated: now,
                access_count: 0,
            })
            .await?;
        node_ids.insert(qualified_name.clone(), node_id.clone());

        if let Some(community_id) = community_id {
            if let Some(community_name) = communities.get(&community_id) {
                let community_ref = format!("{repo_name}:community:{community_id}");
                let community_node_id = KnowledgeGraph::canonical_node_id(
                    &EntityType::Concept,
                    &format!("{repo_name}:community:{community_name}"),
                );
                if seen_community_nodes.insert(community_node_id.clone()) {
                    graph
                        .upsert_node(&GraphNode {
                            id: community_node_id.clone(),
                            label: format!("{repo_name} / {community_name}"),
                            node_type: EntityType::Concept,
                            properties: serde_json::json!({
                                "repo_name": repo_name,
                                "community_id": community_id,
                                "community_name": community_name,
                            }),
                            source_kind: GraphSourceKind::CodeReviewGraph,
                            source_scope: source_scope.to_string(),
                            source_ref: Some(community_ref),
                            confidence: 1.0,
                            created_at: now,
                            last_updated: now,
                            access_count: 0,
                        })
                        .await?;
                }

                graph
                    .add_edge(&GraphEdge {
                        id: KnowledgeGraph::canonical_edge_id(
                            &node_id,
                            &RelationType::RelatedTo,
                            &community_node_id,
                        ),
                        from_id: node_id.clone(),
                        to_id: community_node_id,
                        relation: RelationType::RelatedTo,
                        weight: 1.0,
                        properties: Some(serde_json::json!({
                            "kind": "community_membership",
                            "repo_name": repo_name,
                        })),
                        source_kind: GraphSourceKind::CodeReviewGraph,
                        source_scope: source_scope.to_string(),
                        source_ref: Some(format!("{repo_name}:community:{community_id}")),
                        confidence: 1.0,
                        created_at: now,
                        updated_at: now,
                    })
                    .await?;
            }
        }
    }

    let edge_rows = sqlx::query(
        r#"SELECT kind, source_qualified, target_qualified, file_path, line
           FROM edges"#,
    )
    .fetch_all(&db)
    .await
    .context("Failed to read code-review-graph edges")?;

    let mut edge_count = 0usize;
    for row in edge_rows {
        let source_qn: String = row.get("source_qualified");
        let target_qn: String = row.get("target_qualified");
        let Some(from_id) = node_ids.get(&source_qn).cloned() else {
            continue;
        };
        let Some(to_id) = node_ids.get(&target_qn).cloned() else {
            continue;
        };
        let kind: String = row.get("kind");
        let file_path: String = row.get("file_path");
        let line: i64 = row.get("line");
        graph
            .add_edge(&GraphEdge {
                id: KnowledgeGraph::canonical_edge_id(&from_id, &map_crg_relation(&kind), &to_id),
                from_id,
                to_id,
                relation: map_crg_relation(&kind),
                weight: 1.0,
                properties: Some(serde_json::json!({
                    "repo_name": repo_name,
                    "file_path": file_path,
                    "line": line,
                    "code_review_graph_kind": kind,
                })),
                source_kind: GraphSourceKind::CodeReviewGraph,
                source_scope: source_scope.to_string(),
                source_ref: Some(format!("{repo_name}:{file_path}:{line}")),
                confidence: 1.0,
                created_at: now,
                updated_at: now,
            })
            .await?;
        edge_count += 1;
    }

    sqlx::query(
        r#"INSERT INTO memory_graph_sources
           (source_id, source_kind, source_scope, workspace_path, repo_name, db_path,
            source_last_updated, source_branch, source_commit, last_imported_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(source_id) DO UPDATE SET
             source_kind = excluded.source_kind,
             source_scope = excluded.source_scope,
             workspace_path = excluded.workspace_path,
             repo_name = excluded.repo_name,
             db_path = excluded.db_path,
             source_last_updated = excluded.source_last_updated,
             source_branch = excluded.source_branch,
             source_commit = excluded.source_commit,
             last_imported_at = excluded.last_imported_at"#,
    )
    .bind(format!("crg:{repo_name}"))
    .bind(SOURCE_KIND)
    .bind(source_scope)
    .bind(repo_path.display().to_string())
    .bind(repo_name)
    .bind(db_path.display().to_string())
    .bind(metadata.get("last_updated").cloned())
    .bind(metadata.get("git_branch").cloned())
    .bind(metadata.get("git_head_sha").cloned())
    .bind(now)
    .execute(pool)
    .await?;

    Ok(CodeReviewGraphRepoStatus {
        repo_name: repo_name.to_string(),
        repo_path: repo_path.display().to_string(),
        db_path: db_path.display().to_string(),
        available: true,
        imported: true,
        stale: false,
        nodes: node_ids.len(),
        edges: edge_count,
        files: count_repo_files(&node_ids, repo_name),
        last_updated: metadata.get("last_updated").cloned(),
        built_branch: metadata.get("git_branch").cloned(),
        built_commit: metadata.get("git_head_sha").cloned(),
        current_branch: git_info(repo_path).0,
        current_commit: git_info(repo_path).1,
        message: None,
    })
}

async fn delete_existing_import(pool: &SqlitePool, repo_name: &str) -> Result<()> {
    let prefix = format!("{repo_name}:%");
    sqlx::query("DELETE FROM graph_edges WHERE source_kind = ? AND source_ref LIKE ?")
        .bind(SOURCE_KIND)
        .bind(&prefix)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM graph_nodes WHERE source_kind = ? AND source_ref LIKE ?")
        .bind(SOURCE_KIND)
        .bind(&prefix)
        .execute(pool)
        .await?;
    Ok(())
}

async fn load_communities(db: &SqlitePool) -> Result<HashMap<i64, String>> {
    let exists: Option<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'communities' LIMIT 1",
    )
    .fetch_optional(db)
    .await?;
    if exists.is_none() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query("SELECT id, name FROM communities")
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get::<i64, _>("id"), row.get::<String, _>("name")))
        .collect())
}

fn git_info(repo_path: &Path) -> (Option<String>, Option<String>) {
    let branch = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["branch", "--show-current"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!value.is_empty()).then_some(value)
        });
    let sha = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!value.is_empty()).then_some(value)
        });
    (branch, sha)
}

fn normalized_label(
    repo_name: &str,
    kind: &str,
    name: &str,
    qualified_name: &str,
    file_path: &str,
) -> String {
    match kind {
        "File" => format!("{repo_name}/{}", file_path),
        "Function" | "Class" | "Type" | "Test" => format!("{repo_name}::{qualified_name}"),
        _ => format!("{repo_name}::{name}"),
    }
}

fn map_crg_entity_type(kind: &str, is_test: bool) -> EntityType {
    if is_test {
        return EntityType::Feature;
    }
    match kind {
        "File" => EntityType::File,
        "Function" => EntityType::Function,
        "Class" | "Type" => EntityType::Class,
        "Module" => EntityType::Module,
        _ => EntityType::Concept,
    }
}

fn map_crg_relation(kind: &str) -> RelationType {
    match kind {
        "CALLS" => RelationType::Calls,
        "IMPORTS_FROM" => RelationType::Imports,
        "INHERITS" => RelationType::DependsOn,
        "IMPLEMENTS" => RelationType::ImplementsFor,
        "CONTAINS" => RelationType::Contains,
        "TESTED_BY" => RelationType::TestedBy,
        "DEPENDS_ON" => RelationType::DependsOn,
        other => RelationType::Other(other.to_ascii_lowercase()),
    }
}

fn count_repo_files(node_ids: &HashMap<String, String>, repo_name: &str) -> usize {
    node_ids
        .keys()
        .filter(|qualified_name| qualified_name.starts_with(repo_name))
        .count()
}
