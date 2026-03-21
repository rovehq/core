//! Builtin policy files.
//!
//! These are written to the user policy directory on first load so the user can
//! inspect and override them from a workspace-local `.rove/policy/` folder.

use std::path::Path;

use tokio::fs;
use tracing::info;

pub const GENERAL_TOML: &str = r#"
[meta]
id = "general"
name = "General"
description = "Baseline policy for normal interactive work."
tags = ["default"]
domains = ["general"]

[activation]
priority = 40

[directives]
system_prefix = "For general tasks, answer directly, keep execution grounded in the current workspace, and prefer durable task state over ad-hoc behavior."
"#;

pub const CODE_TOML: &str = r#"
[meta]
id = "code"
name = "Code"
description = "Guidance for code changes and repository work."
tags = ["engineering", "verification"]
domains = ["code"]

[activation]
priority = 70

[directives]
system_prefix = "For code tasks, read the relevant files before editing, keep changes focused, and preserve existing project patterns."
system_suffix = "When code changes are made, run the narrowest useful verification before claiming success."

[hints]
refactor = "After any Rust refactor or code rewrite, run cargo clippy on the affected project before finishing."
rust = "Prefer Rust-idiomatic changes and keep modules small and purpose-named."
"#;

pub const GIT_TOML: &str = r#"
[meta]
id = "git"
name = "Git"
description = "Guidance for repository history and review operations."
tags = ["version-control"]
domains = ["git"]

[activation]
priority = 70

[directives]
system_prefix = "For git tasks, inspect status and diff before mutating history, avoid destructive commands unless explicitly asked, and summarize the resulting repository state."
"#;

pub const SHELL_TOML: &str = r#"
[meta]
id = "shell"
name = "Shell"
description = "Guidance for terminal and filesystem operations."
tags = ["terminal", "filesystem"]
domains = ["shell"]

[activation]
priority = 65

[directives]
system_prefix = "For shell tasks, prefer read-only inspection first, state the working directory when it matters, and keep command execution minimal and explicit."
"#;

pub const SECURITY_TOML: &str = r#"
[meta]
id = "security"
name = "Security"
description = "Always-on safeguards for secrets, sensitive data, and risky operations."
tags = ["security", "privacy"]
domains = ["code", "git", "shell", "general", "browser", "data"]

[activation]
priority = 95
auto_when = ["task contains: password|secret|key|credential|token|pii"]
auto_when_risk_tier = 2

[directives]
system_prefix = "Security policy is active. Never echo secrets back unnecessarily, keep sensitive work local when possible, and prefer refusal over unsafe disclosure."

[routing]
avoid_providers = ["openai", "anthropic", "gemini"]
always_verify = true
min_score_threshold = 0.90

[memory]
auto_tag = ["sensitive", "do-not-share"]
"#;

/// Writes the default built-in policy files to the specified directory if they
/// do not already exist.
pub async fn bootstrap_builtins(policy_dir: &Path) -> anyhow::Result<()> {
    if !policy_dir.exists() {
        fs::create_dir_all(policy_dir).await?;
    }

    let builtins = vec![
        ("general.toml", GENERAL_TOML),
        ("code.toml", CODE_TOML),
        ("git.toml", GIT_TOML),
        ("shell.toml", SHELL_TOML),
        ("security.toml", SECURITY_TOML),
    ];

    for (filename, content) in builtins {
        let path = policy_dir.join(filename);
        if !path.exists() {
            fs::write(&path, content.trim()).await?;
            info!("Bootstrapped built-in policy file: {}", filename);
        }
    }

    Ok(())
}
