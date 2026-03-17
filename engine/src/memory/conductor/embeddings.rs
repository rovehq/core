//! Vector Embeddings Module
//!
//! Generates embeddings for episodic memories using LocalBrain.
//! Embeddings enable semantic search via cosine similarity.

use anyhow::{Context, Result};
use brain::reasoning::LocalBrain;
use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Embedding generator using LocalBrain
pub struct EmbeddingGenerator {
    pool: SqlitePool,
    pub local_brain: Option<Arc<LocalBrain>>,
}

impl EmbeddingGenerator {
    /// Create a new EmbeddingGenerator
    ///
    /// # Arguments
    /// * `pool` - SQLite connection pool
    /// * `local_brain` - Optional LocalBrain instance for embedding generation
    pub fn new(pool: SqlitePool, local_brain: Option<Arc<LocalBrain>>) -> Self {
        Self { pool, local_brain }
    }

    /// Generate embedding for a memory
    ///
    /// # Arguments
    /// * `memory_id` - ID of the memory to embed
    /// * `text` - Text to embed (summary)
    ///
    /// # Returns
    /// Ok(()) if embedding was generated and stored, Err otherwise
    pub async fn generate_embedding(&self, memory_id: &str, text: &str) -> Result<()> {
        // Check if LocalBrain is available
        let brain = match &self.local_brain {
            Some(b) => b,
            None => {
                debug!(memory_id = %memory_id, "LocalBrain not available, skipping embedding");
                return Ok(());
            }
        };

        // Generate embedding
        let embedding = brain.embed(text).await.context("Failed to generate embedding")?;

        // Serialize embedding as bytes
        let embedding_bytes = bincode::serialize(&embedding)
            .context("Failed to serialize embedding")?;

        let now = chrono::Utc::now().timestamp();

        // Store embedding in database
        sqlx::query(
            r#"UPDATE episodic_memory
               SET embedding = ?, embedding_model = 'local-brain', embedding_generated_at = ?
               WHERE id = ?"#,
        )
        .bind(&embedding_bytes)
        .bind(now)
        .bind(memory_id)
        .execute(&self.pool)
        .await
        .context("Failed to store embedding")?;

        debug!(
            memory_id = %memory_id,
            dimensions = embedding.len(),
            "generated and stored embedding"
        );

        Ok(())
    }

    /// Generate embeddings for all memories without embeddings
    ///
    /// Processes up to `limit` memories in a single batch.
    /// Returns the number of embeddings generated.
    pub async fn backfill_embeddings(&self, limit: usize) -> Result<usize> {
        // Check if LocalBrain is available
        if self.local_brain.is_none() {
            debug!("LocalBrain not available, skipping embedding backfill");
            return Ok(0);
        }

        // Fetch memories without embeddings
        let rows = sqlx::query(
            r#"SELECT id, summary
               FROM episodic_memory
               WHERE embedding IS NULL
               LIMIT ?"#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch memories for embedding")?;

        if rows.is_empty() {
            debug!("No memories need embeddings");
            return Ok(0);
        }

        info!(count = rows.len(), "generating embeddings for memories");

        let mut generated = 0;
        for row in rows {
            let memory_id: String = row.get("id");
            let summary: String = row.get("summary");

            match self.generate_embedding(&memory_id, &summary).await {
                Ok(()) => generated += 1,
                Err(e) => {
                    warn!(error = %e, memory_id = %memory_id, "failed to generate embedding");
                }
            }
        }

        info!(generated = generated, "embedding backfill complete");

        Ok(generated)
    }

    /// Calculate cosine similarity between two embedding vectors
    ///
    /// # Arguments
    /// * `a` - First embedding vector
    /// * `b` - Second embedding vector
    ///
    /// # Returns
    /// Cosine similarity score (0.0 to 1.0, higher is more similar)
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if magnitude_a == 0.0 || magnitude_b == 0.0 {
            return 0.0;
        }

        (dot_product / (magnitude_a * magnitude_b)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = EmbeddingGenerator::cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = EmbeddingGenerator::cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = EmbeddingGenerator::cosine_similarity(&a, &b);
        // Opposite vectors should have similarity 0.0 (clamped from -1.0)
        assert!((sim - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0];
        let sim = EmbeddingGenerator::cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }
}
