use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use sqlx::{FromRow, Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct KnowledgeDocument {
    pub id: String,
    pub source_type: String,
    pub source_path: String,
    pub title: Option<String>,
    pub content: String,
    pub content_hash: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub word_count: Option<i64>,
    pub domain: Option<String>,
    pub tags: Option<String>,
    pub indexed_at: i64,
    pub last_accessed: Option<i64>,
    pub access_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeIngestResult {
    pub id: String,
    pub title: Option<String>,
    pub source_type: String,
    pub source_path: String,
    pub word_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeStats {
    pub total_documents: i64,
    pub total_words: i64,
    pub by_source: Vec<SourceBreakdown>,
    pub by_domain: Vec<DomainBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceBreakdown {
    pub source_type: String,
    pub count: i64,
    pub words: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainBreakdown {
    pub domain: Option<String>,
    pub count: i64,
}

#[derive(Clone)]
pub struct KnowledgeRepository {
    pool: SqlitePool,
}

impl KnowledgeRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn ingest(
        &self,
        source_type: &str,
        source_path: &str,
        title: Option<&str>,
        content: &str,
        mime_type: Option<&str>,
        domain: Option<&str>,
        tags: Option<&[&str]>,
    ) -> Result<KnowledgeIngestResult> {
        let id = Uuid::new_v4().to_string();
        let content_hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
        let word_count = content.split_whitespace().count();
        let size_bytes = content.len() as i64;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let tags_json = tags.map(|t| serde_json::to_string(t).unwrap_or_default());

        sqlx::query(
            "INSERT INTO knowledge_documents \
             (id, source_type, source_path, title, content, content_hash, \
              mime_type, size_bytes, word_count, domain, tags, indexed_at, access_count) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)",
        )
        .bind(&id)
        .bind(source_type)
        .bind(source_path)
        .bind(title)
        .bind(content)
        .bind(&content_hash)
        .bind(mime_type)
        .bind(size_bytes)
        .bind(word_count as i64)
        .bind(domain)
        .bind(tags_json)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(KnowledgeIngestResult {
            id,
            title: title.map(String::from),
            source_type: source_type.to_string(),
            source_path: source_path.to_string(),
            word_count,
        })
    }

    pub async fn get(&self, id: &str) -> Result<Option<KnowledgeDocument>> {
        let doc = sqlx::query_as::<_, KnowledgeDocument>(
            "SELECT * FROM knowledge_documents WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if doc.is_some() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            sqlx::query(
                "UPDATE knowledge_documents SET last_accessed = ?, access_count = access_count + 1 WHERE id = ?",
            )
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }

        Ok(doc)
    }

    pub async fn list(&self, limit: usize, offset: usize) -> Result<Vec<KnowledgeDocument>> {
        let docs = sqlx::query_as::<_, KnowledgeDocument>(
            "SELECT * FROM knowledge_documents ORDER BY indexed_at DESC LIMIT ? OFFSET ?",
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(docs)
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<KnowledgeDocument>> {
        let docs = sqlx::query_as::<_, KnowledgeDocument>(
            "SELECT * FROM knowledge_documents \
             WHERE id IN (SELECT rowid FROM knowledge_fts WHERE knowledge_fts MATCH ? LIMIT ?) \
             ORDER BY indexed_at DESC",
        )
        .bind(query)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(docs)
    }

    pub async fn remove(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM knowledge_documents WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn stats(&self) -> Result<KnowledgeStats> {
        let total_documents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_documents")
            .fetch_one(&self.pool)
            .await?;

        let total_words: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(word_count), 0) FROM knowledge_documents")
                .fetch_one(&self.pool)
                .await?;

        let by_source_rows = sqlx::query(
            "SELECT source_type, COUNT(*) as count, COALESCE(SUM(word_count), 0) as words \
             FROM knowledge_documents GROUP BY source_type ORDER BY count DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let by_source: Vec<SourceBreakdown> = by_source_rows
            .into_iter()
            .map(|row| SourceBreakdown {
                source_type: row.get("source_type"),
                count: row.get("count"),
                words: row.get("words"),
            })
            .collect();

        let by_domain_rows = sqlx::query(
            "SELECT domain, COUNT(*) as count \
             FROM knowledge_documents GROUP BY domain ORDER BY count DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let by_domain: Vec<DomainBreakdown> = by_domain_rows
            .into_iter()
            .map(|row| DomainBreakdown {
                domain: row.get("domain"),
                count: row.get("count"),
            })
            .collect();

        Ok(KnowledgeStats {
            total_documents,
            total_words,
            by_source,
            by_domain,
        })
    }

    pub async fn exists_by_path(&self, source_type: &str, source_path: &str) -> Result<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM knowledge_documents WHERE source_type = ? AND source_path = ?",
        )
        .bind(source_type)
        .bind(source_path)
        .fetch_one(&self.pool)
        .await?;

        Ok(count > 0)
    }
}
