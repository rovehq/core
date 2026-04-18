//! Telemetry and Observability
//!
//! Handles setting up `tracing-subscriber` for structured logging.
//! Supports config-driven log levels, environment variable overrides,
//! and format switching between pretty (debug) and JSON (release).

use anyhow::{Context, Result};
use opentelemetry::{global, trace::TracerProvider as _, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{trace::SdkTracerProvider, Resource};
use std::time::Duration;
use tracing::{info_span, Span};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use uuid::Uuid;

/// Per-task trace context for agent/task execution spans.
#[derive(Clone, Debug)]
pub struct TaskTraceContext {
    pub trace_id: String,
    pub root_span_id: String,
}

impl TaskTraceContext {
    pub fn new() -> Self {
        Self {
            trace_id: new_trace_id(),
            root_span_id: new_span_id(),
        }
    }
}

fn new_trace_id() -> String {
    Uuid::new_v4().simple().to_string()
}

fn new_span_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].to_string()
}

/// Create the root task span for a task execution.
pub fn task_span(context: &TaskTraceContext, task_id: &Uuid, task_source: &str) -> Span {
    info_span!(
        "task.run",
        otel_name = "task.run",
        otel_kind = "internal",
        trace_id = %context.trace_id,
        span_id = %context.root_span_id,
        parent_span_id = tracing::field::Empty,
        task_id = %task_id,
        task_source = %task_source
    )
}

/// Create a child span for an LLM round-trip inside a task.
pub fn llm_span(context: &TaskTraceContext, task_id: &Uuid, iteration: usize) -> Span {
    let span_id = new_span_id();
    info_span!(
        "llm.call",
        otel_name = "llm.call",
        otel_kind = "client",
        trace_id = %context.trace_id,
        span_id = %span_id,
        parent_span_id = %context.root_span_id,
        task_id = %task_id,
        iteration
    )
}

/// Create a child span for a tool invocation inside a task.
pub fn tool_span(context: &TaskTraceContext, task_id: &str, tool_name: &str) -> Span {
    let span_id = new_span_id();
    info_span!(
        "tool.call",
        otel_name = "tool.call",
        otel_kind = "client",
        trace_id = %context.trace_id,
        span_id = %span_id,
        parent_span_id = %context.root_span_id,
        task_id = %task_id,
        tool_name = %tool_name
    )
}

fn otlp_timeout_secs() -> u64 {
    std::env::var("ROVE_OTLP_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5)
}

/// Build an OTLP tracer when `ROVE_OTLP_ENDPOINT` is configured.
pub fn otlp_tracer_from_env(service_name: &str) -> Result<Option<opentelemetry_sdk::trace::Tracer>> {
    let Some(endpoint) = std::env::var("ROVE_OTLP_ENDPOINT")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };

    let timeout = Duration::from_secs(otlp_timeout_secs().max(1));
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint.clone())
        .with_timeout(timeout)
        .build()
        .with_context(|| format!("building OTLP exporter for {}", endpoint))?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder_empty()
                .with_attributes(vec![KeyValue::new("service.name", service_name.to_string())])
                .build(),
        )
        .build();
    let tracer = provider.tracer(service_name.to_string());
    global::set_tracer_provider(provider);

    Ok(Some(tracer))
}

/// Initialize the tracing subscriber with the given log level from config.
///
/// Priority: `RUST_LOG` env var > `log_level` parameter > default "info"
///
/// In debug builds: pretty-printed terminal output.
/// In release builds: JSON structured output with spans.
pub fn init_telemetry_with_level(log_level: &str) {
    let default_filter = format!("{},rove_engine={}", log_level, log_level);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&default_filter));
    let otlp_tracer = otlp_tracer_from_env("rove-engine").ok().flatten();

    #[cfg(debug_assertions)]
    {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().pretty().with_target(false))
            .with(otlp_tracer.map(|tracer| tracing_opentelemetry::layer().with_tracer(tracer)))
            .try_init()
            .ok();
    }

    #[cfg(not(debug_assertions))]
    {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json().with_current_span(true))
            .with(otlp_tracer.map(|tracer| tracing_opentelemetry::layer().with_tracer(tracer)))
            .try_init()
            .ok();
    }
}

/// Initialize the tracing subscriber with default settings.
///
/// Falls back to "info" level if no `RUST_LOG` env var is set.
/// Use `init_telemetry_with_level` when config is available.
pub fn init_telemetry() {
    init_telemetry_with_level("info");
}

#[cfg(test)]
mod tests {
    use super::TaskTraceContext;

    #[test]
    fn trace_context_uses_otel_sized_ids() {
        let context = TaskTraceContext::new();
        assert_eq!(context.trace_id.len(), 32);
        assert_eq!(context.root_span_id.len(), 16);
    }
}
