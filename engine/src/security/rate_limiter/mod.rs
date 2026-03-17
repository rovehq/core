//! Rate limiting module
//!
//! This module provides rate limiting functionality to prevent abuse and
//! protect against overwhelming the system. It tracks operations by source
//! (e.g., user ID, chat ID) and risk tier, enforcing different limits based
//! on the tier:
//!
//! - **Tier 1 (Medium Risk)**: 60 operations per hour
//! - **Tier 2 (High Risk)**: 10 operations per 10 minutes AND 5 operations per 60 seconds
//!
//! # Circuit Breaker
//!
//! For Tier 2 operations, a circuit breaker trips when 5 operations occur
//! within 60 seconds. When tripped, all Tier 2 operations require local unlock.
//!
//! # Database Tracking
//!
//! All operations are tracked in the `rate_limits` table with automatic cleanup
//! of old entries (older than 1 hour).
//!
//! Requirements: 11.1, 11.2, 11.3, 11.4, 11.5, 11.6, 11.7

use anyhow::{Context, Result};
use sdk::errors::EngineError;
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

use crate::risk_assessor::RiskTier;

/// Rate limiter for tracking and enforcing operation limits
pub struct RateLimiter {
    pool: SqlitePool,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Check if an operation is allowed under rate limits
    ///
    /// This checks the appropriate limits based on the risk tier:
    /// - Tier 0: No limits (always allowed)
    /// - Tier 1: 60 operations per hour
    /// - Tier 2: 10 operations per 10 minutes AND 5 operations per 60 seconds
    ///
    /// Requirements: 11.1, 11.2, 11.3
    pub async fn check_limit(&self, source: &str, tier: RiskTier) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get current time")?
            .as_millis() as u64;

        match tier {
            RiskTier::Tier0 => {
                // No rate limiting for read-only operations
                debug!("Tier 0 operation - no rate limit");
                Ok(())
            }
            RiskTier::Tier1 => {
                // 60 operations per hour
                let count = self.count_operations(source, 1, now - 3_600_000).await?;
                debug!(
                    "Tier 1 rate limit check: {}/60 operations in last hour",
                    count
                );

                if count >= 60 {
                    warn!(
                        "Rate limit exceeded for source {} (Tier 1): {}/60",
                        source, count
                    );
                    return Err(EngineError::RateLimitExceeded {
                        src: source.to_string(),
                        tier: 1,
                        count,
                        limit: 60,
                        window: "1 hour".to_string(),
                    }
                    .into());
                }
                Ok(())
            }
            RiskTier::Tier2 => {
                // Check 10 operations per 10 minutes
                let count_10m = self.count_operations(source, 2, now - 600_000).await?;
                debug!(
                    "Tier 2 rate limit check (10min): {}/10 operations",
                    count_10m
                );

                if count_10m >= 10 {
                    warn!(
                        "Rate limit exceeded for source {} (Tier 2, 10min): {}/10",
                        source, count_10m
                    );
                    return Err(EngineError::RateLimitExceeded {
                        src: source.to_string(),
                        tier: 2,
                        count: count_10m,
                        limit: 10,
                        window: "10 minutes".to_string(),
                    }
                    .into());
                }

                // Check 5 operations per 60 seconds (circuit breaker threshold)
                let count_1m = self.count_operations(source, 2, now - 60_000).await?;
                debug!("Tier 2 rate limit check (60sec): {}/5 operations", count_1m);

                if count_1m >= 5 {
                    // Trip circuit breaker
                    self.trip_circuit_breaker(source, now).await?;
                    error!(
                        "Circuit breaker tripped for source {}: {}/5 operations in 60 seconds",
                        source, count_1m
                    );
                    return Err(EngineError::CircuitBreakerTripped {
                        src: source.to_string(),
                        count: count_1m,
                    }
                    .into());
                }

                Ok(())
            }
        }
    }

    /// Record an operation for rate limiting
    ///
    /// This should be called after an operation is successfully executed.
    /// It records the operation in the database and cleans up old entries.
    ///
    /// Requirements: 11.1, 11.2, 11.3
    pub async fn record_operation(&self, source: &str, tier: RiskTier) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get current time")?
            .as_millis() as u64;

        // Only record Tier 1 and Tier 2 operations
        if matches!(tier, RiskTier::Tier0) {
            return Ok(());
        }

        let tier_value = tier as i32;
        let now_i64 = now as i64;

        sqlx::query("INSERT INTO rate_limits (source, tier, timestamp) VALUES (?, ?, ?)")
            .bind(source)
            .bind(tier_value)
            .bind(now_i64)
            .execute(&self.pool)
            .await
            .context("Failed to record operation")?;

        debug!(
            "Recorded operation: source={}, tier={}, timestamp={}",
            source, tier_value, now
        );

        // Clean up old entries (older than 1 hour)
        self.cleanup_old_entries(now).await?;

        Ok(())
    }

    /// Count operations for a source and tier since a given timestamp
    ///
    /// Requirements: 11.1, 11.2, 11.3
    async fn count_operations(&self, source: &str, tier: i32, since: u64) -> Result<i64> {
        let since_i64 = since as i64;

        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM rate_limits WHERE source = ? AND tier = ? AND timestamp >= ?",
        )
        .bind(source)
        .bind(tier)
        .bind(since_i64)
        .fetch_one(&self.pool)
        .await
        .context("Failed to count operations")?;

        Ok(result.0)
    }

    /// Trip the circuit breaker for a source
    ///
    /// This logs the circuit breaker trip with timestamp and source for audit.
    ///
    /// Requirements: 11.4, 11.5, 11.6
    async fn trip_circuit_breaker(&self, source: &str, timestamp: u64) -> Result<()> {
        error!(
            "CIRCUIT BREAKER TRIPPED: source={}, timestamp={}, reason=5 Tier 2 operations in 60 seconds",
            source, timestamp
        );

        // Log to database for audit trail
        // We use a special tier value (-1) to mark circuit breaker trips
        let timestamp_i64 = timestamp as i64;

        sqlx::query("INSERT INTO rate_limits (source, tier, timestamp) VALUES (?, ?, ?)")
            .bind(source)
            .bind(-1)
            .bind(timestamp_i64)
            .execute(&self.pool)
            .await
            .context("Failed to log circuit breaker trip")?;

        info!("Circuit breaker trip logged for source: {}", source);
        Ok(())
    }

    /// Clean up old rate limit entries
    ///
    /// Removes entries older than 1 hour to prevent unbounded growth.
    ///
    /// Requirements: 11.7
    async fn cleanup_old_entries(&self, now: u64) -> Result<()> {
        let cutoff = (now - 3_600_000) as i64; // 1 hour ago

        let result = sqlx::query("DELETE FROM rate_limits WHERE timestamp < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .context("Failed to clean up old entries")?;

        if result.rows_affected() > 0 {
            debug!(
                "Cleaned up {} old rate limit entries",
                result.rows_affected()
            );
        }

        Ok(())
    }

    /// Check if circuit breaker is tripped for a source
    ///
    /// This checks if there's a recent circuit breaker trip (within last 5 minutes).
    ///
    /// Requirements: 11.4, 11.5
    pub async fn is_circuit_breaker_tripped(&self, source: &str) -> Result<bool> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get current time")?
            .as_millis() as u64;

        let five_minutes_ago = (now - 300_000) as i64;

        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM rate_limits WHERE source = ? AND tier = -1 AND timestamp >= ?",
        )
        .bind(source)
        .bind(five_minutes_ago)
        .fetch_one(&self.pool)
        .await
        .context("Failed to check circuit breaker status")?;

        Ok(result.0 > 0)
    }

    /// Reset circuit breaker for a source
    ///
    /// This should be called when a local unlock is performed.
    ///
    /// Requirements: 11.5
    pub async fn reset_circuit_breaker(&self, source: &str) -> Result<()> {
        sqlx::query("DELETE FROM rate_limits WHERE source = ? AND tier = -1")
            .bind(source)
            .execute(&self.pool)
            .await
            .context("Failed to reset circuit breaker")?;

        info!("Circuit breaker reset for source: {}", source);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tempfile::TempDir;

    async fn setup_test_db() -> (TempDir, Database, RateLimiter) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::new(&db_path).await.unwrap();
        let limiter = RateLimiter::new(db.pool().clone());
        (temp_dir, db, limiter)
    }

    #[tokio::test]
    async fn test_tier0_no_limit() {
        let (_temp_dir, _db, limiter) = setup_test_db().await;

        // Tier 0 operations should never be rate limited
        for _ in 0..100 {
            assert!(limiter
                .check_limit("test_source", RiskTier::Tier0)
                .await
                .is_ok());
            limiter
                .record_operation("test_source", RiskTier::Tier0)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn test_tier1_hourly_limit() {
        let (_temp_dir, _db, limiter) = setup_test_db().await;

        // Should allow up to 60 operations
        for i in 0..60 {
            assert!(
                limiter
                    .check_limit("test_source", RiskTier::Tier1)
                    .await
                    .is_ok(),
                "Operation {} should be allowed",
                i
            );
            limiter
                .record_operation("test_source", RiskTier::Tier1)
                .await
                .unwrap();
            // Small delay to ensure unique timestamps
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        }

        // 61st operation should be blocked
        assert!(limiter
            .check_limit("test_source", RiskTier::Tier1)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_tier2_10min_limit() {
        let (_temp_dir, _db, limiter) = setup_test_db().await;

        // For testing the 10-minute limit without hitting the 60-second circuit breaker,
        // we need to insert operations with timestamps spread across 10 minutes
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Insert 10 operations spread across the last 10 minutes
        // Each operation is 1 minute apart to avoid the 60-second circuit breaker
        for i in 0..10 {
            let timestamp = (now - (i as u64 * 60_000)) as i64; // 1 minute apart
            sqlx::query("INSERT INTO rate_limits (source, tier, timestamp) VALUES (?, ?, ?)")
                .bind("test_source")
                .bind(2)
                .bind(timestamp)
                .execute(&limiter.pool)
                .await
                .unwrap();
        }

        // Now we have 10 operations in the last 10 minutes
        // The 11th operation should be blocked by the 10-minute limit
        assert!(limiter
            .check_limit("test_source", RiskTier::Tier2)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_tier2_circuit_breaker() {
        let (_temp_dir, _db, limiter) = setup_test_db().await;

        // Should allow up to 5 operations in 60 seconds
        for i in 0..5 {
            assert!(
                limiter
                    .check_limit("test_source", RiskTier::Tier2)
                    .await
                    .is_ok(),
                "Operation {} should be allowed",
                i
            );
            limiter
                .record_operation("test_source", RiskTier::Tier2)
                .await
                .unwrap();
            // Small delay to ensure unique timestamps
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        }

        // 6th operation should trip circuit breaker
        let result = limiter.check_limit("test_source", RiskTier::Tier2).await;
        assert!(result.is_err());

        // Verify circuit breaker is tripped
        assert!(limiter
            .is_circuit_breaker_tripped("test_source")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_circuit_breaker_reset() {
        let (_temp_dir, _db, limiter) = setup_test_db().await;

        // Trip the circuit breaker
        for _ in 0..5 {
            limiter
                .check_limit("test_source", RiskTier::Tier2)
                .await
                .ok();
            limiter
                .record_operation("test_source", RiskTier::Tier2)
                .await
                .unwrap();
            // Small delay to ensure unique timestamps
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        }

        // Verify it's tripped
        assert!(limiter
            .check_limit("test_source", RiskTier::Tier2)
            .await
            .is_err());
        assert!(limiter
            .is_circuit_breaker_tripped("test_source")
            .await
            .unwrap());

        // Reset the circuit breaker
        limiter.reset_circuit_breaker("test_source").await.unwrap();

        // Verify it's reset
        assert!(!limiter
            .is_circuit_breaker_tripped("test_source")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_separate_sources() {
        let (_temp_dir, _db, limiter) = setup_test_db().await;

        // Fill up rate limit for source1
        for _ in 0..60 {
            limiter.check_limit("source1", RiskTier::Tier1).await.ok();
            limiter
                .record_operation("source1", RiskTier::Tier1)
                .await
                .unwrap();
            // Small delay to ensure unique timestamps
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        }

        // source1 should be blocked
        assert!(limiter
            .check_limit("source1", RiskTier::Tier1)
            .await
            .is_err());

        // source2 should still be allowed
        assert!(limiter
            .check_limit("source2", RiskTier::Tier1)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_cleanup_old_entries() {
        let (_temp_dir, _db, limiter) = setup_test_db().await;

        // Record some operations
        for _ in 0..10 {
            limiter
                .record_operation("test_source", RiskTier::Tier1)
                .await
                .unwrap();
            // Small delay to ensure unique timestamps
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        }

        // Manually insert an old entry (2 hours ago)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let old_timestamp = (now - 7_200_000) as i64; // 2 hours ago

        sqlx::query("INSERT INTO rate_limits (source, tier, timestamp) VALUES (?, ?, ?)")
            .bind("test_source")
            .bind(1)
            .bind(old_timestamp)
            .execute(&limiter.pool)
            .await
            .unwrap();

        // Trigger cleanup by recording a new operation
        limiter
            .record_operation("test_source", RiskTier::Tier1)
            .await
            .unwrap();

        // Verify old entry was cleaned up
        let cutoff = (now - 3_600_000) as i64;
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM rate_limits WHERE timestamp < ?")
            .bind(cutoff)
            .fetch_one(&limiter.pool)
            .await
            .unwrap();

        assert_eq!(count.0, 0, "Old entries should be cleaned up");
    }
}
