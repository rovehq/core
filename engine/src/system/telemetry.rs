//! Telemetry and Observability
//!
//! Handles setting up `tracing-subscriber` for structured logging.
//! Supports config-driven log levels, environment variable overrides,
//! and format switching between pretty (debug) and JSON (release).

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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

    #[cfg(debug_assertions)]
    {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().pretty().with_target(false))
            .try_init()
            .ok();
    }

    #[cfg(not(debug_assertions))]
    {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json().with_current_span(true))
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
