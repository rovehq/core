//! Tests for system::metrics — render_prometheus(), MetricsSnapshot

use rove_engine::system::metrics::{render_prometheus, MetricsSnapshot};

// ── MetricsSnapshot default ───────────────────────────────────────────────────

#[test]
fn default_snapshot_all_zeros() {
    let snap = MetricsSnapshot::default();
    assert_eq!(snap.tasks_total, 0);
    assert_eq!(snap.tasks_pending, 0);
    assert_eq!(snap.tasks_running, 0);
    assert_eq!(snap.tasks_completed, 0);
    assert_eq!(snap.tasks_failed, 0);
    assert_eq!(snap.tool_calls_total, 0);
    assert_eq!(snap.task_error_rate, 0.0);
    assert_eq!(snap.task_latency_avg_ms, 0.0);
    assert_eq!(snap.active_sessions, 0);
}

#[test]
fn default_snapshot_eq_empty() {
    let snap = MetricsSnapshot::default();
    assert_eq!(snap, MetricsSnapshot::default());
}

#[test]
fn snapshot_clone_equals_original() {
    let snap = MetricsSnapshot {
        tasks_total: 10,
        tasks_pending: 1,
        tasks_running: 2,
        tasks_completed: 5,
        tasks_failed: 2,
        tool_calls_total: 42,
        task_error_rate: 0.286,
        task_latency_avg_ms: 1234.5,
        active_sessions: 3,
    };
    let cloned = snap.clone();
    assert_eq!(snap, cloned);
}

// ── render_prometheus() format ────────────────────────────────────────────────

#[test]
fn render_contains_tasks_total_metric() {
    let snap = MetricsSnapshot {
        tasks_total: 5,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_tasks_total 5"));
}

#[test]
fn render_contains_tasks_pending_metric() {
    let snap = MetricsSnapshot {
        tasks_pending: 3,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_tasks_pending 3"));
}

#[test]
fn render_contains_tasks_running_metric() {
    let snap = MetricsSnapshot {
        tasks_running: 2,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_tasks_running 2"));
}

#[test]
fn render_contains_tasks_completed_metric() {
    let snap = MetricsSnapshot {
        tasks_completed: 99,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_tasks_completed_total 99"));
}

#[test]
fn render_contains_tasks_failed_metric() {
    let snap = MetricsSnapshot {
        tasks_failed: 7,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_tasks_failed_total 7"));
}

#[test]
fn render_contains_tool_calls_metric() {
    let snap = MetricsSnapshot {
        tool_calls_total: 100,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_tool_calls_total 100"));
}

#[test]
fn render_contains_error_rate_metric() {
    let snap = MetricsSnapshot {
        task_error_rate: 0.25,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_task_error_rate 0.250000"));
}

#[test]
fn render_contains_latency_metric() {
    let snap = MetricsSnapshot {
        task_latency_avg_ms: 500.0,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_task_latency_avg_ms 500.000"));
}

#[test]
fn render_contains_active_sessions_metric() {
    let snap = MetricsSnapshot {
        active_sessions: 4,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_active_sessions 4"));
}

// ── Prometheus format structure ────────────────────────────────────────────────

#[test]
fn render_contains_help_comments() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("# HELP"));
}

#[test]
fn render_contains_type_comments() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("# TYPE"));
}

#[test]
fn render_tasks_total_is_counter() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("# TYPE rove_tasks_total counter"));
}

#[test]
fn render_tasks_pending_is_gauge() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("# TYPE rove_tasks_pending gauge"));
}

#[test]
fn render_tasks_running_is_gauge() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("# TYPE rove_tasks_running gauge"));
}

#[test]
fn render_error_rate_is_gauge() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("# TYPE rove_task_error_rate gauge"));
}

#[test]
fn render_latency_is_gauge() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("# TYPE rove_task_latency_avg_ms gauge"));
}

#[test]
fn render_sessions_is_gauge() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("# TYPE rove_active_sessions gauge"));
}

// ── Value formatting ─────────────────────────────────────────────────────────

#[test]
fn render_zero_error_rate_format() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_task_error_rate 0.000000"));
}

#[test]
fn render_zero_latency_format() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_task_latency_avg_ms 0.000"));
}

#[test]
fn render_50pct_error_rate() {
    let snap = MetricsSnapshot {
        task_error_rate: 0.5,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("0.500000"));
}

#[test]
fn render_100pct_error_rate() {
    let snap = MetricsSnapshot {
        task_error_rate: 1.0,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("1.000000"));
}

#[test]
fn render_large_numbers() {
    let snap = MetricsSnapshot {
        tasks_total: 1_000_000,
        tool_calls_total: 5_000_000,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("1000000"));
    assert!(rendered.contains("5000000"));
}

#[test]
fn render_negative_would_still_format() {
    // Shouldn't happen but test edge case
    let snap = MetricsSnapshot {
        tasks_total: -1,
        ..Default::default()
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("-1"));
}

// ── All metrics present in single render call ─────────────────────────────────

#[test]
fn render_contains_all_metric_names() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    let expected_metrics = [
        "rove_tasks_total",
        "rove_tasks_pending",
        "rove_tasks_running",
        "rove_tasks_completed_total",
        "rove_tasks_failed_total",
        "rove_tool_calls_total",
        "rove_task_error_rate",
        "rove_task_latency_avg_ms",
        "rove_active_sessions",
    ];
    for metric in &expected_metrics {
        assert!(rendered.contains(metric), "Missing metric: {}", metric);
    }
}

#[test]
fn render_output_is_newline_separated() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains('\n'));
}

#[test]
fn render_output_not_empty() {
    let snap = MetricsSnapshot::default();
    let rendered = render_prometheus(&snap);
    assert!(!rendered.is_empty());
}

// ── Complex scenario ─────────────────────────────────────────────────────────

#[test]
fn render_realistic_scenario() {
    let snap = MetricsSnapshot {
        tasks_total: 1000,
        tasks_pending: 5,
        tasks_running: 3,
        tasks_completed: 950,
        tasks_failed: 42,
        tool_calls_total: 5000,
        task_error_rate: 0.0423,
        task_latency_avg_ms: 1250.75,
        active_sessions: 12,
    };
    let rendered = render_prometheus(&snap);
    assert!(rendered.contains("rove_tasks_total 1000"));
    assert!(rendered.contains("rove_tasks_pending 5"));
    assert!(rendered.contains("rove_tasks_running 3"));
    assert!(rendered.contains("rove_tasks_completed_total 950"));
    assert!(rendered.contains("rove_tasks_failed_total 42"));
    assert!(rendered.contains("rove_tool_calls_total 5000"));
    assert!(rendered.contains("rove_active_sessions 12"));
}
