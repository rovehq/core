use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::storage::knowledge::KnowledgeIngestResult;
use crate::storage::{KnowledgeRepository, MAX_INGEST_BYTES};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchResult {
    pub url: String,
    pub status: u16,
    pub content_type: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestSummary {
    pub total: usize,
    pub ingested: Vec<KnowledgeIngestResult>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

pub async fn ingest_file(
    repo: &KnowledgeRepository,
    path: &Path,
    domain: Option<&str>,
    tags: Option<&[&str]>,
    force: bool,
    ingested_by: Option<&str>,
) -> Result<KnowledgeIngestResult> {
    let source_path = path.display().to_string();
    let source_type = "file";

    if !force && repo.exists_by_path(source_type, &source_path).await? {
        bail!("Already indexed: {} (use --force to reindex)", source_path);
    }

    let meta = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("Failed to stat {}", source_path))?;
    if meta.len() as usize > MAX_INGEST_BYTES {
        bail!(
            "File too large: {} ({} bytes, limit {} MiB)",
            source_path,
            meta.len(),
            MAX_INGEST_BYTES / 1024 / 1024
        );
    }

    let content = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read {}", source_path))?;

    let title = path.file_stem().and_then(|s| s.to_str()).map(title_case);
    let mime_type = detect_mime(path);

    repo.ingest(
        source_type,
        &source_path,
        title.as_deref(),
        &content,
        mime_type.as_deref(),
        domain,
        tags,
        ingested_by,
    )
    .await
}

pub async fn ingest_folder(
    repo: &KnowledgeRepository,
    dir: &Path,
    domain: Option<&str>,
    tags: Option<&[&str]>,
    force: bool,
    dry_run: bool,
    ingested_by: Option<&str>,
) -> Result<IngestSummary> {
    let mut summary = IngestSummary {
        total: 0,
        ingested: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    };

    let mut files = Vec::new();
    collect_files(dir, &mut files)?;
    summary.total = files.len();

    for path in files {
        if dry_run {
            summary
                .skipped
                .push(format!("[dry-run] {}", path.display()));
            continue;
        }

        match ingest_file(repo, &path, domain, tags, force, ingested_by).await {
            Ok(result) => summary.ingested.push(result),
            Err(e) => summary.errors.push(format!("{}: {}", path.display(), e)),
        }
    }

    Ok(summary)
}

pub async fn ingest_url(
    repo: &KnowledgeRepository,
    url: &str,
    domain: Option<&str>,
    tags: Option<&[&str]>,
    force: bool,
    ingested_by: Option<&str>,
) -> Result<KnowledgeIngestResult> {
    if !force && repo.exists_by_path("url", url).await? {
        bail!("Already indexed: {} (use --force to reindex)", url);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(url).send().await?;
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let bytes = resp.bytes().await?;
    if bytes.len() > MAX_INGEST_BYTES {
        bail!(
            "URL content too large: {} bytes (limit {} MiB)",
            bytes.len(),
            MAX_INGEST_BYTES / 1024 / 1024
        );
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let content = if content_type.as_deref().unwrap_or("").contains("text/html") {
        html_to_text(&text)
    } else {
        text
    };

    let title = extract_title(&content, url);

    repo.ingest(
        "url",
        url,
        Some(&title),
        &content,
        content_type.as_deref(),
        domain,
        tags,
        ingested_by,
    )
    .await
}

pub async fn fetch_url_text(url: &str, max_chars: usize) -> Result<WebFetchResult> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(url).send().await?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let text = resp.text().await?;
    let mut content = if content_type.as_deref().unwrap_or("").contains("text/html") {
        html_to_text(&text)
    } else {
        text
    };
    if max_chars > 0 && content.chars().count() > max_chars {
        content = content.chars().take(max_chars).collect();
    }

    Ok(WebFetchResult {
        url: url.to_string(),
        status,
        content_type,
        content,
    })
}

pub async fn ingest_sitemap(
    repo: &KnowledgeRepository,
    sitemap_url: &str,
    domain: Option<&str>,
    tags: Option<&[&str]>,
    force: bool,
    dry_run: bool,
    ingested_by: Option<&str>,
) -> Result<IngestSummary> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(sitemap_url).send().await?;
    let xml = resp.text().await?;

    let urls = parse_sitemap_urls(&xml);
    let mut summary = IngestSummary {
        total: urls.len(),
        ingested: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    };

    for url in urls {
        if dry_run {
            summary.skipped.push(format!("[dry-run] {}", url));
            continue;
        }

        match ingest_url(repo, &url, domain, tags, force, ingested_by).await {
            Ok(result) => summary.ingested.push(result),
            Err(e) => summary.errors.push(format!("{}: {}", url, e)),
        }
    }

    Ok(summary)
}

// ── Helpers ───────────────────────────────────────────────────────

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md", "txt", "rst", "json", "toml", "yaml", "yml", "csv", "html", "htm", "xml", "log", "py",
    "rs", "ts", "js", "go", "java", "c", "cpp", "h", "hpp", "sh", "bash", "zsh", "sql",
];

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(
                name,
                ".git"
                    | "node_modules"
                    | "target"
                    | "dist"
                    | "build"
                    | ".next"
                    | "__pycache__"
                    | ".venv"
                    | "vendor"
            ) {
                continue;
            }
            collect_files(&path, out)?;
        } else if is_supported_file(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_supported_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn detect_mime(path: &Path) -> Option<String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("md") => Some("text/markdown".to_string()),
        Some("json") => Some("application/json".to_string()),
        Some("toml") => Some("application/toml".to_string()),
        Some("yaml") | Some("yml") => Some("application/yaml".to_string()),
        Some("html") | Some("htm") => Some("text/html".to_string()),
        Some("xml") => Some("application/xml".to_string()),
        Some("csv") => Some("text/csv".to_string()),
        _ => Some("text/plain".to_string()),
    }
}

fn title_case(value: &str) -> String {
    value
        .split(&['-', '_', ' '])
        .filter(|s| !s.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str().to_lowercase()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn html_to_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
            continue;
        }
        if ch == '>' {
            in_tag = false;
            text.push(' ');
            continue;
        }
        if !in_tag {
            text.push(ch);
        }
    }
    let mut result = String::new();
    let mut prev_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            prev_space = false;
            result.push(ch);
        }
    }
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{fetch_url_text, html_to_text};
    use axum::{routing::get, Router};

    #[test]
    fn html_to_text_strips_tags() {
        let text = html_to_text("<html><body><h1>Hello</h1><p>world</p></body></html>");
        assert_eq!(text, "Hello world");
    }

    #[tokio::test]
    async fn fetch_url_text_converts_html_to_text() {
        let app = Router::new().route(
            "/",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    "<html><body><h1>Hello</h1><p>world</p></body></html>",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let result = fetch_url_text(&format!("http://{}/", addr), 1024)
            .await
            .unwrap();
        assert_eq!(result.status, 200);
        assert_eq!(result.content, "Hello world");

        server.abort();
    }
}

fn extract_title(content: &str, url: &str) -> String {
    if let Some(start) = content.find("<title>") {
        if let Some(end) = content[start..].find("</title>") {
            let title = &content[start + 7..start + end];
            if !title.trim().is_empty() {
                return title.trim().to_string();
            }
        }
    }
    url.split('/').last().unwrap_or(url).to_string()
}

fn parse_sitemap_urls(xml: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for line in xml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<loc>") && trimmed.ends_with("</loc>") {
            let url = &trimmed[5..trimmed.len() - 6];
            if !url.is_empty() {
                urls.push(url.to_string());
            }
        }
    }
    urls
}
