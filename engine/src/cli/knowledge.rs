use anyhow::Result;

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::Database;
use crate::system::knowledge::{self, IngestSummary};

use super::commands::{KnowledgeAction, KnowledgeIngestSource};

pub async fn handle_knowledge(action: KnowledgeAction, config: &Config) -> Result<()> {
    let db = Database::new(&database_path(config)).await?;
    let repo = db.knowledge();

    match action {
        KnowledgeAction::Ingest {
            source,
            domain,
            tags,
            force,
            dry_run,
        } => {
            handle_ingest(&repo, source, domain, tags, force, dry_run).await?;
        }
        KnowledgeAction::List { limit, offset } => {
            handle_list(&repo, limit, offset).await?;
        }
        KnowledgeAction::Show { id } => {
            handle_show(&repo, &id).await?;
        }
        KnowledgeAction::Search { query, limit } => {
            handle_search(&repo, &query, limit).await?;
        }
        KnowledgeAction::Remove { id } => {
            handle_remove(&repo, &id).await?;
        }
        KnowledgeAction::Stats => {
            handle_stats(&repo).await?;
        }
    }

    Ok(())
}

async fn handle_ingest(
    repo: &crate::storage::KnowledgeRepository,
    source: KnowledgeIngestSource,
    domain: Option<String>,
    tags: Option<Vec<String>>,
    force: bool,
    dry_run: bool,
) -> Result<()> {
    let domain_ref = domain.as_deref();
    let tags_vec: Vec<&str> = tags
        .as_ref()
        .map(|t| t.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();
    let tags_ref: Option<&[&str]> = if tags_vec.is_empty() {
        None
    } else {
        Some(&tags_vec)
    };

    let summary = match source {
        KnowledgeIngestSource::File { path } => {
            let result =
                knowledge::ingest_file(repo, &path, domain_ref, tags_ref, force, Some("cli"))
                    .await?;
            print_ingest_result(&IngestSummary {
                total: 1,
                ingested: vec![result],
                skipped: Vec::new(),
                errors: Vec::new(),
            });
            return Ok(());
        }
        KnowledgeIngestSource::Folder { path } => {
            knowledge::ingest_folder(
                repo, &path, domain_ref, tags_ref, force, dry_run, Some("cli"),
            )
            .await?
        }
        KnowledgeIngestSource::Url { url } => {
            let result =
                knowledge::ingest_url(repo, &url, domain_ref, tags_ref, force, Some("cli"))
                    .await?;
            print_ingest_result(&IngestSummary {
                total: 1,
                ingested: vec![result],
                skipped: Vec::new(),
                errors: Vec::new(),
            });
            return Ok(());
        }
        KnowledgeIngestSource::Sitemap { url } => {
            knowledge::ingest_sitemap(
                repo, &url, domain_ref, tags_ref, force, dry_run, Some("cli"),
            )
            .await?
        }
    };

    print_ingest_result(&summary);
    Ok(())
}

async fn handle_list(
    repo: &crate::storage::KnowledgeRepository,
    limit: usize,
    offset: usize,
) -> Result<()> {
    let docs = repo.list(limit, offset).await?;
    let count = docs.len();
    if docs.is_empty() {
        println!("No knowledge documents.");
        return Ok(());
    }

    println!(
        "{:<38} {:<8} {:<12} {:<6} {}",
        "ID", "Source", "Domain", "Words", "Title"
    );
    println!("{}", "-".repeat(100));
    for doc in docs {
        let id_short = &doc.id[..doc.id.len().min(36)];
        let title = doc.title.as_deref().unwrap_or("(untitled)");
        println!(
            "{:<38} {:<8} {:<12} {:<6} {}",
            id_short,
            doc.source_type,
            doc.domain.as_deref().unwrap_or("—"),
            doc.word_count.unwrap_or(0),
            title
        );
    }
    println!("\n{} document(s) shown.", count);
    Ok(())
}

async fn handle_show(repo: &crate::storage::KnowledgeRepository, id: &str) -> Result<()> {
    match repo.get(id).await? {
        Some(doc) => {
            let content_preview = if doc.content.len() > 500 {
                format!("{}...", &doc.content[..500])
            } else {
                doc.content.clone()
            };

            println!("ID:           {}", doc.id);
            println!("Source type:  {}", doc.source_type);
            println!("Source path:  {}", doc.source_path);
            println!(
                "Title:        {}",
                doc.title.as_deref().unwrap_or("(untitled)")
            );
            println!("Domain:       {}", doc.domain.as_deref().unwrap_or("—"));
            println!("Words:        {}", doc.word_count.unwrap_or(0));
            println!("MIME type:    {}", doc.mime_type.as_deref().unwrap_or("—"));
            println!("Indexed at:   {}", doc.indexed_at);
            println!("Access count: {}", doc.access_count);
            println!();
            println!("--- Content ---");
            println!("{}", content_preview);
        }
        None => {
            println!("Knowledge document '{}' not found.", id);
        }
    }
    Ok(())
}

async fn handle_search(
    repo: &crate::storage::KnowledgeRepository,
    query: &str,
    limit: usize,
) -> Result<()> {
    let hits = repo.search(query, limit).await?;
    let count = hits.len();
    if hits.is_empty() {
        println!("No results for '{}'.", query);
        return Ok(());
    }

    println!("Search results for '{}':", query);
    println!(
        "{:<38} {:<8} {:<12} {:<6} {}",
        "ID", "Source", "Domain", "Words", "Title"
    );
    println!("{}", "-".repeat(100));
    for hit in hits {
        let doc = &hit.doc;
        let id_short = &doc.id[..doc.id.len().min(36)];
        let title = doc.title.as_deref().unwrap_or("(untitled)");
        println!(
            "{:<38} {:<8} {:<12} {:<6} {}",
            id_short,
            doc.source_type,
            doc.domain.as_deref().unwrap_or("—"),
            doc.word_count.unwrap_or(0),
            title
        );
        if !hit.snippet.is_empty() {
            println!("  {}", hit.snippet);
        }
    }
    println!("\n{} result(s) found.", count);
    Ok(())
}

async fn handle_remove(repo: &crate::storage::KnowledgeRepository, id: &str) -> Result<()> {
    if repo.remove(id).await? {
        println!("Removed knowledge document '{}'.", id);
    } else {
        println!("Knowledge document '{}' not found.", id);
    }
    Ok(())
}

async fn handle_stats(repo: &crate::storage::KnowledgeRepository) -> Result<()> {
    let stats = repo.stats().await?;
    println!("Knowledge Base Statistics");
    println!("=========================");
    println!("  Total documents: {}", stats.total_documents);
    println!("  Total words:     {}", stats.total_words);
    println!();

    if !stats.by_source.is_empty() {
        println!("  By source:");
        for s in stats.by_source {
            println!(
                "    {:<12} {} docs, {} words",
                s.source_type, s.count, s.words
            );
        }
        println!();
    }

    if !stats.by_domain.is_empty() {
        println!("  By domain:");
        for d in stats.by_domain {
            let domain = d.domain.as_deref().unwrap_or("(unset)");
            println!("    {:<12} {} docs", domain, d.count);
        }
    }
    Ok(())
}

fn print_ingest_result(summary: &IngestSummary) {
    println!("Ingest Summary");
    println!("==============");
    println!("  Total sources:  {}", summary.total);
    println!("  Ingested:       {}", summary.ingested.len());
    println!("  Skipped:        {}", summary.skipped.len());
    println!("  Errors:         {}", summary.errors.len());

    if !summary.ingested.is_empty() {
        println!();
        println!("  Ingested documents:");
        for r in &summary.ingested {
            let title = r.title.as_deref().unwrap_or("(untitled)");
            println!(
                "    {}  ({}) — {} words",
                title, r.source_type, r.word_count
            );
        }
    }

    if !summary.skipped.is_empty() {
        println!();
        println!("  Skipped:");
        for s in &summary.skipped {
            println!("    {}", s);
        }
    }

    if !summary.errors.is_empty() {
        println!();
        println!("  Errors:");
        for e in &summary.errors {
            println!("    {}", e);
        }
    }
}
