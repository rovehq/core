use std::time::{SystemTime, UNIX_EPOCH};

/// Returns current Unix timestamp as i64
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Returns recency decay multiplier based on memory age
/// 0-1d: 1.0, 2-7d: 0.9, 8-30d: 0.7, 31-90d: 0.5, 90+d: 0.3
pub fn recency_decay(created_at: i64, now: i64) -> f32 {
    let age_days = (now - created_at).max(0) / 86_400;
    match age_days {
        0..=1 => 1.0,
        2..=7 => 0.9,
        8..=30 => 0.7,
        31..=90 => 0.5,
        _ => 0.3,
    }
}

/// Calculates final relevance score from a raw FTS5 BM25 rank.
///
/// FTS5 BM25 is **negative** (more negative = better match). This function
/// negates it so the final score is positive and higher = more relevant,
/// consistent with the descending sort used at all call sites.
pub fn score(bm25: f32, importance: f32, created_at: i64, now: i64) -> f32 {
    (-bm25) * importance * recency_decay(created_at, now)
}

/// Returns true if importance meets minimum threshold
pub fn should_inject(importance: f32, min: f32) -> bool {
    importance >= min
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recency_decay_fresh() {
        let now = unix_now();
        assert_eq!(recency_decay(now, now), 1.0);
        assert_eq!(recency_decay(now - 86_400, now), 1.0); // 1 day
    }

    #[test]
    fn test_recency_decay_week() {
        let now = unix_now();
        assert_eq!(recency_decay(now - 2 * 86_400, now), 0.9); // 2 days
        assert_eq!(recency_decay(now - 7 * 86_400, now), 0.9); // 7 days
    }

    #[test]
    fn test_recency_decay_month() {
        let now = unix_now();
        assert_eq!(recency_decay(now - 30 * 86_400, now), 0.7); // 30 days
    }

    #[test]
    fn test_recency_decay_old() {
        let now = unix_now();
        assert_eq!(recency_decay(now - 90 * 86_400, now), 0.5); // 90 days
        assert_eq!(recency_decay(now - 365 * 86_400, now), 0.3); // 1 year
    }

    #[test]
    fn test_score_calculation() {
        let now = unix_now();
        // FTS5 BM25 is negative; score() negates it so the result is positive.
        let score_val = score(-10.0, 0.8, now, now);
        assert_eq!(score_val, 8.0); // (-(-10.0)) * 0.8 * 1.0
    }

    #[test]
    fn test_should_inject() {
        assert!(should_inject(0.5, 0.4));
        assert!(!should_inject(0.3, 0.4));
        assert!(should_inject(0.4, 0.4));
    }
}
