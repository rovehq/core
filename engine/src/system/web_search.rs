use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::{SearchConfig, SearchProviderKind, SearxngSearchConfig};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchResultItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source_engine: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchResult {
    pub query: String,
    pub provider: String,
    pub results: Vec<WebSearchResultItem>,
}

#[async_trait]
pub trait SearchProvider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<WebSearchResultItem>>;
}

pub async fn search_web(
    config: &SearchConfig,
    query: &str,
    limit: usize,
    provider_override: Option<&str>,
) -> Result<WebSearchResult> {
    let query = query.trim();
    if query.is_empty() {
        bail!("web_search requires a non-empty query");
    }

    let provider_kind = provider_override
        .map(parse_provider_kind)
        .transpose()?
        .unwrap_or_else(|| config.provider.clone());

    let provider = provider_from_config(config, provider_kind)?;
    let results = provider.search(query, limit.max(1)).await?;
    Ok(WebSearchResult {
        query: query.to_string(),
        provider: provider.name().to_string(),
        results,
    })
}

fn provider_from_config(
    config: &SearchConfig,
    provider_kind: SearchProviderKind,
) -> Result<Box<dyn SearchProvider>> {
    match provider_kind {
        SearchProviderKind::Disabled => bail!("web_search provider is disabled in config"),
        SearchProviderKind::Searxng => Ok(Box::new(SearxngProvider::new(config.searxng.clone()))),
    }
}

fn parse_provider_kind(raw: &str) -> Result<SearchProviderKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => bail!("provider override cannot be empty"),
        "disabled" | "none" | "off" => Ok(SearchProviderKind::Disabled),
        "searxng" | "searx" => Ok(SearchProviderKind::Searxng),
        other => bail!("unsupported search provider '{}'", other),
    }
}

struct SearxngProvider {
    config: SearxngSearchConfig,
    client: reqwest::Client,
}

impl SearxngProvider {
    fn new(config: SearxngSearchConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("valid SearxNG reqwest client");
        Self { config, client }
    }

    fn endpoint(&self) -> Result<reqwest::Url> {
        let base = self.config.base_url.trim_end_matches('/');
        let endpoint = format!("{}/search", base);
        reqwest::Url::parse(&endpoint)
            .with_context(|| format!("invalid SearxNG base URL '{}'", self.config.base_url))
    }
}

#[async_trait]
impl SearchProvider for SearxngProvider {
    fn name(&self) -> &'static str {
        "searxng"
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<WebSearchResultItem>> {
        let mut endpoint = self.endpoint()?;
        endpoint
            .query_pairs_mut()
            .append_pair("q", query)
            .append_pair("format", "json")
            .append_pair("language", "all")
            .append_pair("safesearch", "0");

        let response = self.client.get(endpoint).send().await?;
        let status = response.status();
        if !status.is_success() {
            bail!("SearxNG search failed with HTTP {}", status.as_u16());
        }

        let body: SearxngSearchResponse = response.json().await?;
        Ok(body
            .results
            .into_iter()
            .take(limit)
            .map(normalize_searxng_result)
            .collect())
    }
}

#[derive(Debug, Deserialize)]
struct SearxngSearchResponse {
    #[serde(default)]
    results: Vec<SearxngSearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearxngSearchItem {
    #[serde(default)]
    title: String,
    url: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    engine: Option<String>,
    #[serde(default)]
    engines: Vec<String>,
}

fn normalize_searxng_result(item: SearxngSearchItem) -> WebSearchResultItem {
    let source_engine = item
        .engine
        .filter(|value| !value.is_empty())
        .or_else(|| item.engines.into_iter().find(|value| !value.is_empty()))
        .unwrap_or_else(|| "searxng".to_string());

    WebSearchResultItem {
        title: if item.title.trim().is_empty() {
            item.url.clone()
        } else {
            item.title
        },
        url: item.url,
        snippet: item.content,
        source_engine,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_override_aliases() {
        assert!(matches!(
            parse_provider_kind("searx"),
            Ok(SearchProviderKind::Searxng)
        ));
        assert!(matches!(
            parse_provider_kind("disabled"),
            Ok(SearchProviderKind::Disabled)
        ));
        assert!(parse_provider_kind("duckduckgo").is_err());
    }

    #[test]
    fn normalizes_searxng_result() {
        let item = SearxngSearchItem {
            title: String::new(),
            url: "https://example.com".to_string(),
            content: "Example snippet".to_string(),
            engine: None,
            engines: vec!["google".to_string()],
        };

        let normalized = normalize_searxng_result(item);
        assert_eq!(normalized.title, "https://example.com");
        assert_eq!(normalized.url, "https://example.com");
        assert_eq!(normalized.snippet, "Example snippet");
        assert_eq!(normalized.source_engine, "google");
    }
}
