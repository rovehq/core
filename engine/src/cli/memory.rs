use anyhow::Result;

use crate::config::{Config, MemoryMode};
use crate::system::memory::{MemoryIngestRequest, MemoryManager, MemoryQueryRequest};

use super::commands::{
    MemoryAction, MemoryAdapterAction, MemoryAlwaysOnAction, MemoryGraphAction, MemoryModeAction,
    MemoryModeArg,
};

pub async fn handle_memory(action: MemoryAction) -> Result<()> {
    let config = Config::load_or_create()?;
    let manager = MemoryManager::new(config);

    match action {
        MemoryAction::Status => print_status(&manager.status().await?),
        MemoryAction::Mode { action } => match action {
            MemoryModeAction::Set { mode } => {
                print_status(&manager.set_mode(parse_mode(mode)).await?)
            }
        },
        MemoryAction::Query {
            explain,
            domain,
            question,
        } => {
            let response = manager
                .query(MemoryQueryRequest {
                    question: question.join(" "),
                    explain,
                    domain,
                })
                .await?;
            print_query(response);
        }
        MemoryAction::Graph { action } => match action {
            MemoryGraphAction::Inspect { entity } => {
                let response = manager.inspect_graph(entity).await?;
                println!(
                    "Graph health: healthy={} available={} imported={} stale={}",
                    response.graph_status.healthy,
                    response.graph_status.available_count,
                    response.graph_status.imported_count,
                    response.graph_status.stale_count
                );
                println!(
                    "Graph stats: nodes={} edges={}",
                    response
                        .graph_stats
                        .get("nodes")
                        .copied()
                        .unwrap_or_default(),
                    response
                        .graph_stats
                        .get("edges")
                        .copied()
                        .unwrap_or_default()
                );
                if !response.paths.is_empty() {
                    println!("Paths:");
                    for path in response.paths {
                        println!(
                            "- {} [{}] confidence={:.2}",
                            path.summary,
                            path.source_kinds.join(", "),
                            path.confidence
                        );
                    }
                }
            }
        },
        MemoryAction::Reindex => print_status(&manager.reindex().await?),
        MemoryAction::Backfill { batch } => {
            let embedded = manager.backfill_embeddings(batch).await?;
            println!("Backfilled embeddings: {}", embedded);
        }
        MemoryAction::Ingest { domain, note } => {
            let hit = manager
                .ingest_note(MemoryIngestRequest {
                    note: note.join(" "),
                    domain,
                })
                .await?;
            println!("Ingested note: {}", hit.content);
        }
        MemoryAction::Adapters { action } => match action {
            MemoryAdapterAction::List => print_status(&manager.status().await?),
            MemoryAdapterAction::Refresh => print_status(&manager.reindex().await?),
        },
        MemoryAction::AlwaysOn { action } => match action {
            MemoryAlwaysOnAction::Enable => {
                print_status(&manager.set_mode(MemoryMode::AlwaysOn).await?)
            }
            MemoryAlwaysOnAction::Disable => {
                print_status(&manager.set_mode(MemoryMode::GraphOnly).await?)
            }
        },
    }

    Ok(())
}

fn parse_mode(mode: MemoryModeArg) -> MemoryMode {
    match mode {
        MemoryModeArg::GraphOnly => MemoryMode::GraphOnly,
        MemoryModeArg::AlwaysOn => MemoryMode::AlwaysOn,
    }
}

fn print_status(status: &crate::system::memory::MemorySurfaceStatus) {
    println!(
        "Memory mode: {} | bundle_strategy={} | retrieval_assist={} | graph_enrichment={} | scope={}",
        status.mode,
        status.bundle_strategy,
        status.retrieval_assist,
        status.graph_enrichment,
        status.scope
    );
    println!(
        "Persistence: pinned_facts={} task_traces={} | code_adapter_mode={}",
        status.persist_pinned_facts, status.persist_task_traces, status.code_adapter_mode
    );
    println!(
        "Graph status: healthy={} available={} imported={} stale={}",
        status.graph_status.healthy,
        status.graph_status.available_count,
        status.graph_status.imported_count,
        status.graph_status.stale_count
    );
    println!(
        "Graph stats: nodes={} edges={}",
        status.graph_stats.get("nodes").copied().unwrap_or_default(),
        status.graph_stats.get("edges").copied().unwrap_or_default()
    );
    println!(
        "Memory stats: facts={} task_traces={} episodic={} insights={} mem_edges={} embeddings={}/{} ({:.1}%)",
        status.memory_stats.facts,
        status.memory_stats.task_traces,
        status.memory_stats.episodic,
        status.memory_stats.insights,
        status.memory_stats.memory_graph_edges,
        status.memory_stats.embedded_episodic,
        status.memory_stats.total_episodic,
        status.memory_stats.embedding_coverage_pct
    );
    if !status.memory_stats.edge_types.is_empty() {
        println!("Memory graph edge types:");
        for (edge_type, count) in &status.memory_stats.edge_types {
            println!("- {}={}", edge_type, count);
        }
    }
    if !status.warnings.is_empty() {
        println!("Warnings:");
        for warning in &status.warnings {
            println!("- {}", warning);
        }
    }
    if !status.graph_status.repos.is_empty() {
        println!("Repos:");
        for repo in &status.graph_status.repos {
            println!(
                "- {} available={} imported={} stale={} nodes={} edges={}",
                repo.repo_name, repo.available, repo.imported, repo.stale, repo.nodes, repo.edges
            );
        }
    }
}

fn print_query(response: crate::system::memory::MemoryQueryResponse) {
    if let Some(project_context) = response.project_context.as_deref() {
        println!("Project context:\n{}", project_context);
    }
    print_hit_group("Facts", &response.facts);
    print_hit_group("Preferences", &response.preferences);
    print_hit_group("Warnings", &response.warnings);
    print_hit_group("Errors", &response.errors);
    if !response.graph_paths.is_empty() {
        println!("Graph paths:");
        for path in &response.graph_paths {
            println!(
                "- {} [{}] confidence={:.2}",
                path.summary,
                path.source_kinds.join(", "),
                path.confidence
            );
        }
    }
    print_hit_group("Insights", &response.insight_hits);
    print_hit_group("Episodes", &response.episodic_hits);
    print_hit_group("Task traces", &response.task_trace_hits);
    if !response.memory_graph_hits.is_empty() {
        println!("Memory graph hits:");
        for hit in &response.memory_graph_hits {
            let edges = if hit.path_edge_types.is_empty() {
                "linked".to_string()
            } else {
                hit.path_edge_types.join(" -> ")
            };
            println!(
                "- depth={} score={:.2} via {} :: {}",
                hit.depth, hit.graph_score, edges, hit.content
            );
        }
    }

    if let Some(explain) = response.explain {
        println!(
            "Explain: intent={} mode={} sources={} graph_paths={} memory_graph_hits={} task_traces={} llm_enrichment={}{}",
            explain.intent,
            explain.mode,
            explain.sources.join(", "),
            explain.graph_paths_used,
            explain.memory_graph_hits_used,
            explain.task_trace_hits_used,
            explain.llm_enrichment_enabled,
            explain
                .fallback_reason
                .as_deref()
                .map(|reason| format!(" fallback={reason}"))
                .unwrap_or_default()
        );
    }
}

fn print_hit_group(label: &str, hits: &[crate::conductor::MemoryHit]) {
    if hits.is_empty() {
        return;
    }
    println!("{label}:");
    for hit in hits {
        println!("- [{}] {}", hit.source, hit.content);
    }
}
