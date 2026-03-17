//! Builtin Steering Skills
//!
//! Provides the raw TOML strings for the default skills that ship with Rove.
//! These are automatically written to `~/.rove/steering/` if they don't exist.

use std::path::Path;
use tokio::fs;
use tracing::info;

pub const CAREFUL_TOML: &str = r#"
[meta]
id = "careful"
name = "Careful Mode"
description = "Thorough and cautious execution. Favors explicit planning and strict verification."
tags = ["quality", "thorough"]

[activation]
manual = true
priority = 50
conflicts_with = ["fast"]

[directives]
system_prefix = "You are operating in Careful Mode. Before executing any action, think through all edge cases. If modifying files, double-check paths and ensure you are not breaking existing functionality."
system_suffix = "Always verify your work after completing a step."

[directives.per_stage]
Plan = "Break tasks down into small, explicit steps. Identify potential risks."
Execute = "Take your time. If a command fails, investigate the root cause before retrying."
Verify = "Run comprehensive tests. Do not assume success."

[routing]
preferred_providers = ["claude-opus", "claude-sonnet"]
always_verify = true
min_score_threshold = 0.80
"#;

pub const FAST_TOML: &str = r#"
[meta]
id = "fast"
name = "Fast Mode"
description = "Quick execution. Skips extensive planning for simple tasks."
tags = ["speed", "agile"]

[activation]
manual = true
priority = 50
conflicts_with = ["careful", "deep-research"]

[directives]
system_prefix = "You are operating in Fast Mode. Optimize for speed and direct action. Omit unnecessary explanations."

[directives.per_stage]
Plan = "Keep the plan brief. Execute immediately if the path is clear."
Execute = "Perform actions directly."

[routing]
prefer_mode = "fast"
min_score_threshold = 0.50
"#;

pub const CODE_REVIEW_TOML: &str = r#"
[meta]
id = "code-review"
name = "Code Reviewer"
description = "Specializes in analyzing code for bugs, style, and security."
tags = ["review", "security", "style"]

[activation]
manual = true
priority = 60
auto_when = ["task contains: review|audit|PR|diff"]

[directives]
system_prefix = "You are acting as a senior code reviewer. Look for logical errors, security vulnerabilities, performance bottlenecks, and style violations."

[tools]
prefer = ["read_file", "search_code"]
"#;

pub const DEEP_RESEARCH_TOML: &str = r#"
[meta]
id = "deep-research"
name = "Deep Researcher"
description = "Thoroughly explores codebases and documentation before acting."
tags = ["research", "analysis"]

[activation]
manual = true
priority = 60
conflicts_with = ["fast"]
auto_when = ["task contains: investigate|research|deep dive"]

[directives]
system_prefix = "You are in Deep Research mode. Prioritize gathering context over taking immediate action. Use search tools extensively."

[tools]
prefer = ["search_code", "read_file", "web_search"]
"#;

pub const LOCAL_ONLY_TOML: &str = r#"
[meta]
id = "local-only"
name = "Local Only"
description = "Forces execution to use local models only. Hardblocks cloud providers."
tags = ["privacy", "offline"]

[activation]
manual = true
priority = 100

[directives]
system_prefix = "CRITICAL: You are running in a strict local-only environment. Do not attempt to use cloud services or external APIs."

[routing]
avoid_providers = ["anthropic", "openai", "google"]
"#;

pub const SENSITIVE_TOML: &str = r#"
[meta]
id = "sensitive"
name = "Sensitive Data Guard"
description = "Activates automatically when sensitive data is detected. Restricts actions and reporting."
tags = ["security", "privacy", "guard"]

[activation]
priority = 100
auto_when = ["task contains: password|secret|key|credential|token|pii"]
auto_when_risk_tier = 2

[directives]
system_prefix = "SECURITY PROTOCOL ACTIVE: You are handling sensitive data. Do not log, echo, or transmit secrets outside of explicit safe channels. Prioritize local processing."

[routing]
preferred_providers = ["local"]
always_verify = true
min_score_threshold = 0.90

[memory]
auto_tag = ["sensitive", "do-not-share"]
"#;

/// Writes the default built-in skills to the specified directory if they don't exist.
pub async fn bootstrap_builtins(skills_dir: &Path) -> anyhow::Result<()> {
    if !skills_dir.exists() {
        fs::create_dir_all(skills_dir).await?;
    }

    let builtins = vec![
        ("careful.toml", CAREFUL_TOML),
        ("fast.toml", FAST_TOML),
        ("code-review.toml", CODE_REVIEW_TOML),
        ("deep-research.toml", DEEP_RESEARCH_TOML),
        ("local-only.toml", LOCAL_ONLY_TOML),
        ("sensitive.toml", SENSITIVE_TOML),
    ];

    for (filename, content) in builtins {
        let path = skills_dir.join(filename);
        if !path.exists() {
            fs::write(&path, content.trim()).await?;
            info!("Bootstrapped built-in skill: {}", filename);
        }
    }

    Ok(())
}
