use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tracing::warn;

use crate::config::Config;
use sdk::errors::EngineError;
use sdk::TaskSource;

const DEFAULT_TIMEOUT_SECS: u64 = 5;
const FAILURE_THRESHOLD: u32 = 3;
const COOLDOWN_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
enum HookEvent {
    #[serde(rename = "BeforeToolCall")]
    BeforeToolCall,
    #[serde(rename = "AfterToolCall")]
    AfterToolCall,
}

impl HookEvent {
    fn as_str(self) -> &'static str {
        match self {
            HookEvent::BeforeToolCall => "BeforeToolCall",
            HookEvent::AfterToolCall => "AfterToolCall",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct HookRequires {
    #[serde(default)]
    os: Vec<String>,
    #[serde(default)]
    bins: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookRequirementsSummary {
    pub os: Vec<String>,
    pub bins: Vec<String>,
    pub env: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HookDefinition {
    name: String,
    #[serde(default)]
    description: Option<String>,
    events: Vec<HookEvent>,
    command: String,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    requires: HookRequires,
}

#[derive(Debug, Default)]
struct HookState {
    consecutive_failures: u32,
    disabled_until: Option<Instant>,
}

#[derive(Debug)]
struct HookHandle {
    definition: HookDefinition,
    command_dir: PathBuf,
    state: Mutex<HookState>,
}

#[derive(Clone, Default)]
pub struct HookManager {
    before_tool_call: Vec<Arc<HookHandle>>,
    after_tool_call: Vec<Arc<HookHandle>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookSummary {
    pub name: String,
    pub description: Option<String>,
    pub events: Vec<String>,
    pub command: String,
    pub timeout_secs: u64,
    pub source_path: String,
    pub requires: HookRequirementsSummary,
    pub consecutive_failures: u32,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookStatus {
    pub hooks: Vec<HookSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BeforeToolCallPayload {
    pub event: &'static str,
    pub task_id: String,
    pub tool_name: String,
    pub args: Value,
    pub task_source: String,
    pub workspace: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AfterToolCallPayload {
    pub event: &'static str,
    pub task_id: String,
    pub tool_name: String,
    pub args: Value,
    pub result: Value,
    pub task_source: String,
    pub workspace: String,
}

#[derive(Debug)]
pub struct BeforeToolCallOutcome {
    pub args: Value,
    pub modified_by: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct HookResponse {
    action: Option<String>,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    reason: Option<String>,
}

enum BeforeHookResult {
    Continue,
    Modify(Value),
    Block(String),
}

impl HookManager {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn discover(config: &Config) -> Self {
        let mut discovered: HashMap<String, Arc<HookHandle>> = HashMap::new();
        for root in hook_roots(config) {
            for handle in discover_root(&root) {
                discovered
                    .entry(handle.definition.name.clone())
                    .or_insert_with(|| Arc::new(handle));
            }
        }

        let mut manager = Self::default();
        for handle in discovered.into_values() {
            if handle
                .definition
                .events
                .contains(&HookEvent::BeforeToolCall)
            {
                manager.before_tool_call.push(Arc::clone(&handle));
            }
            if handle.definition.events.contains(&HookEvent::AfterToolCall) {
                manager.after_tool_call.push(handle);
            }
        }
        manager
            .before_tool_call
            .sort_by(|left, right| left.definition.name.cmp(&right.definition.name));
        manager
            .after_tool_call
            .sort_by(|left, right| left.definition.name.cmp(&right.definition.name));
        manager
    }

    pub async fn before_tool_call(
        &self,
        payload: BeforeToolCallPayload,
    ) -> Result<BeforeToolCallOutcome, EngineError> {
        let mut args = payload.args.clone();
        let mut modified_by = Vec::new();

        for hook in &self.before_tool_call {
            let mut next_payload = payload.clone();
            next_payload.args = args.clone();

            match hook.run_before_tool_call(next_payload).await {
                Ok(BeforeHookResult::Continue) => {}
                Ok(BeforeHookResult::Modify(updated_args)) => {
                    args = updated_args;
                    modified_by.push(hook.definition.name.clone());
                }
                Ok(BeforeHookResult::Block(reason)) => {
                    return Err(EngineError::ToolError(format!(
                        "tool call blocked by hook '{}': {}",
                        hook.definition.name, reason
                    )));
                }
                Err(error) => {
                    warn!(
                        hook = %hook.definition.name,
                        error = %error,
                        "BeforeToolCall hook failed"
                    );
                }
            }
        }

        Ok(BeforeToolCallOutcome { args, modified_by })
    }

    pub async fn after_tool_call(&self, payload: AfterToolCallPayload) {
        let mut jobs = JoinSet::new();
        for hook in &self.after_tool_call {
            let hook = Arc::clone(hook);
            let payload = payload.clone();
            jobs.spawn(async move {
                if let Err(error) = hook.run_after_tool_call(payload).await {
                    warn!(
                        hook = %hook.definition.name,
                        error = %error,
                        "AfterToolCall hook failed"
                    );
                }
            });
        }

        while jobs.join_next().await.is_some() {}
    }

    pub async fn status(&self) -> HookStatus {
        let mut hooks = Vec::new();
        for handle in self
            .before_tool_call
            .iter()
            .chain(self.after_tool_call.iter())
        {
            if hooks
                .iter()
                .any(|existing: &HookSummary| existing.name == handle.definition.name)
            {
                continue;
            }
            hooks.push(handle.summary().await);
        }
        hooks.sort_by(|left, right| left.name.cmp(&right.name));
        HookStatus { hooks }
    }

    pub async fn inspect(&self, name: &str) -> Option<HookSummary> {
        for handle in self
            .before_tool_call
            .iter()
            .chain(self.after_tool_call.iter())
        {
            if handle.definition.name == name {
                return Some(handle.summary().await);
            }
        }
        None
    }
}

impl HookHandle {
    async fn summary(&self) -> HookSummary {
        let state = self.state.lock().await;
        HookSummary {
            name: self.definition.name.clone(),
            description: self.definition.description.clone(),
            events: self
                .definition
                .events
                .iter()
                .map(|event| event.as_str().to_string())
                .collect(),
            command: self.definition.command.clone(),
            timeout_secs: self.definition.timeout.unwrap_or(DEFAULT_TIMEOUT_SECS),
            source_path: self.command_dir.join("HOOK.md").display().to_string(),
            requires: HookRequirementsSummary {
                os: self.definition.requires.os.clone(),
                bins: self.definition.requires.bins.clone(),
                env: self.definition.requires.env.clone(),
            },
            consecutive_failures: state.consecutive_failures,
            disabled: state
                .disabled_until
                .is_some_and(|until| Instant::now() < until),
        }
    }

    async fn run_before_tool_call(
        &self,
        payload: BeforeToolCallPayload,
    ) -> anyhow::Result<BeforeHookResult> {
        let output = self.run_json_hook(&payload).await?;
        if !output.success {
            return Ok(BeforeHookResult::Block(output.reason()));
        }

        let Some(response) = output.response else {
            return Ok(BeforeHookResult::Continue);
        };

        match response.action.as_deref() {
            Some("modify") => {
                let data = response
                    .data
                    .ok_or_else(|| anyhow::anyhow!("hook returned modify without data"))?;
                let args = match data {
                    Value::Object(mut object) if object.contains_key("args") => {
                        object.remove("args").unwrap_or(Value::Null)
                    }
                    value => value,
                };
                Ok(BeforeHookResult::Modify(args))
            }
            Some("block") => Ok(BeforeHookResult::Block(
                response
                    .reason
                    .unwrap_or_else(|| "blocked by hook".to_string()),
            )),
            Some("continue") | None => Ok(BeforeHookResult::Continue),
            Some(other) => Err(anyhow::anyhow!("unsupported hook action '{}'", other)),
        }
    }

    async fn run_after_tool_call(&self, payload: AfterToolCallPayload) -> anyhow::Result<()> {
        let output = self.run_json_hook(&payload).await?;
        if !output.success {
            return Err(anyhow::anyhow!(output.reason()));
        }
        Ok(())
    }

    async fn run_json_hook<T: Serialize>(
        &self,
        payload: &T,
    ) -> anyhow::Result<HookInvocationOutput> {
        {
            let mut state = self.state.lock().await;
            if let Some(disabled_until) = state.disabled_until {
                if Instant::now() < disabled_until {
                    return Err(anyhow::anyhow!(
                        "hook temporarily disabled by circuit breaker"
                    ));
                }
                state.disabled_until = None;
                state.consecutive_failures = 0;
            }
        }

        let timeout_secs = self.definition.timeout.unwrap_or(DEFAULT_TIMEOUT_SECS);
        let mut command = shell_command(&self.definition.command);
        command.current_dir(&self.command_dir);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| {
            anyhow::anyhow!(
                "failed to spawn hook '{}' command '{}': {}",
                self.definition.name,
                self.definition.command,
                error
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            let input = serde_json::to_vec(payload)?;
            stdin.write_all(&input).await?;
        }

        let output =
            match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
                .await
            {
                Ok(result) => result?,
                Err(_) => {
                    self.register_failure().await;
                    return Err(anyhow::anyhow!(
                        "hook '{}' timed out after {}s",
                        self.definition.name,
                        timeout_secs
                    ));
                }
            };

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let parsed = if stdout.is_empty() {
            None
        } else {
            match serde_json::from_str::<HookResponse>(&stdout) {
                Ok(parsed) => Some(parsed),
                Err(error) => {
                    self.register_failure().await;
                    return Err(anyhow::anyhow!("invalid hook stdout JSON: {}", error));
                }
            }
        };

        let outcome = HookInvocationOutput {
            success: output.status.success(),
            stdout,
            stderr,
            response: parsed,
        };

        if outcome.success {
            self.reset_failures().await;
        } else if output.status.code() == Some(1) {
            self.reset_failures().await;
        } else {
            self.register_failure().await;
        }

        Ok(outcome)
    }

    async fn register_failure(&self) {
        let mut state = self.state.lock().await;
        state.consecutive_failures += 1;
        if state.consecutive_failures >= FAILURE_THRESHOLD {
            state.disabled_until = Some(Instant::now() + Duration::from_secs(COOLDOWN_SECS));
            state.consecutive_failures = 0;
        }
    }

    async fn reset_failures(&self) {
        let mut state = self.state.lock().await;
        state.consecutive_failures = 0;
        state.disabled_until = None;
    }
}

struct HookInvocationOutput {
    success: bool,
    stdout: String,
    stderr: String,
    response: Option<HookResponse>,
}

impl HookInvocationOutput {
    fn reason(&self) -> String {
        if let Some(response) = &self.response {
            if let Some(reason) = &response.reason {
                return reason.clone();
            }
        }
        if !self.stderr.is_empty() {
            return self.stderr.clone();
        }
        if !self.stdout.is_empty() {
            return self.stdout.clone();
        }
        "hook rejected operation".to_string()
    }
}

fn hook_roots(config: &Config) -> Vec<PathBuf> {
    let mut roots = vec![config.core.workspace.join(".rove").join("hooks")];
    if let Ok(config_path) = Config::config_path() {
        if let Some(config_dir) = config_path.parent() {
            roots.push(config_dir.join("hooks"));
        }
    }
    roots
}

fn discover_root(root: &Path) -> Vec<HookHandle> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut handles = Vec::new();
    for entry in entries.flatten() {
        let hook_dir = entry.path();
        let hook_file = hook_dir.join("HOOK.md");
        if !hook_file.is_file() {
            continue;
        }

        match load_hook(&hook_file) {
            Ok(Some(handle)) => handles.push(handle),
            Ok(None) => {}
            Err(error) => {
                warn!(path = %hook_file.display(), error = %error, "Failed to load hook");
            }
        }
    }
    handles
}

fn load_hook(path: &Path) -> anyhow::Result<Option<HookHandle>> {
    let contents = std::fs::read_to_string(path)?;
    let definition: HookDefinition = toml::from_str(&contents)?;
    if !hook_is_eligible(&definition.requires) {
        return Ok(None);
    }
    Ok(Some(HookHandle {
        definition,
        command_dir: path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("hook file missing parent directory"))?
            .to_path_buf(),
        state: Mutex::new(HookState::default()),
    }))
}

fn hook_is_eligible(requires: &HookRequires) -> bool {
    if !requires.os.is_empty() {
        let current = env::consts::OS;
        if !requires
            .os
            .iter()
            .any(|value| value.eq_ignore_ascii_case(current))
        {
            return false;
        }
    }

    if requires
        .env
        .iter()
        .any(|key| env::var_os(key).filter(|value| !value.is_empty()).is_none())
    {
        return false;
    }

    if requires.bins.iter().any(|bin| !binary_available(bin)) {
        return false;
    }

    true
}

fn binary_available(bin: &str) -> bool {
    if bin.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(bin).is_file();
    }

    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|path| binary_exists_in_dir(&path, bin))
}

fn binary_exists_in_dir(dir: &Path, bin: &str) -> bool {
    let candidate = dir.join(bin);
    if candidate.is_file() {
        return true;
    }

    #[cfg(windows)]
    {
        for ext in [".exe", ".bat", ".cmd"] {
            if dir.join(format!("{bin}{ext}")).is_file() {
                return true;
            }
        }
    }

    false
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }

    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-lc").arg(command);
        cmd
    }
}

pub fn task_source_label(source: &TaskSource) -> String {
    match source {
        TaskSource::Cli => "cli".to_string(),
        TaskSource::Telegram(user_id) => format!("telegram:{user_id}"),
        TaskSource::Channel(kind) => format!("channel:{kind}"),
        TaskSource::WebUI => "webui".to_string(),
        TaskSource::Remote(node) => format!("remote:{node}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_hook(root: &Path, name: &str, hook_body: &str, script_body: &str) -> PathBuf {
        let hook_dir = root.join(name);
        std::fs::create_dir_all(&hook_dir).expect("hook dir");
        let script_path = hook_dir.join("handler.sh");
        std::fs::write(&script_path, script_body).expect("script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&script_path, perms).expect("chmod");
        }
        std::fs::write(hook_dir.join("HOOK.md"), hook_body).expect("hook file");
        hook_dir
    }

    fn sample_before_payload() -> BeforeToolCallPayload {
        BeforeToolCallPayload {
            event: "BeforeToolCall",
            task_id: "task-1".to_string(),
            tool_name: "run_command".to_string(),
            args: serde_json::json!({ "command": "echo hello" }),
            task_source: "cli".to_string(),
            workspace: "/tmp/workspace".to_string(),
        }
    }

    #[tokio::test]
    async fn before_tool_call_can_modify_arguments() {
        let temp = TempDir::new().expect("tempdir");
        let hooks_root = temp.path().join(".rove").join("hooks");
        write_hook(
            &hooks_root,
            "rewrite-command",
            r#"
name = "rewrite-command"
events = ["BeforeToolCall"]
command = "./handler.sh"
timeout = 2
"#,
            "#!/bin/sh\nprintf '{\"action\":\"modify\",\"data\":{\"args\":{\"command\":\"echo rewritten\"}}}'\n",
        );

        let config = Config {
            core: crate::config::CoreConfig {
                workspace: temp.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let manager = HookManager::discover(&config);
        let result = manager
            .before_tool_call(sample_before_payload())
            .await
            .expect("before result");
        assert_eq!(result.args["command"], "echo rewritten");
        assert_eq!(result.modified_by, vec!["rewrite-command".to_string()]);
    }

    #[tokio::test]
    async fn before_tool_call_can_block() {
        let temp = TempDir::new().expect("tempdir");
        let hooks_root = temp.path().join(".rove").join("hooks");
        write_hook(
            &hooks_root,
            "block-command",
            r#"
name = "block-command"
events = ["BeforeToolCall"]
command = "./handler.sh"
"#,
            "#!/bin/sh\nprintf 'dangerous command'\nexit 1\n",
        );

        let config = Config {
            core: crate::config::CoreConfig {
                workspace: temp.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let manager = HookManager::discover(&config);
        let error = manager
            .before_tool_call(sample_before_payload())
            .await
            .expect_err("blocked");
        assert!(error.to_string().contains("block-command"));
    }

    #[tokio::test]
    async fn failing_hook_trips_circuit_breaker() {
        let temp = TempDir::new().expect("tempdir");
        let hooks_root = temp.path().join(".rove").join("hooks");
        write_hook(
            &hooks_root,
            "flaky-hook",
            r#"
name = "flaky-hook"
events = ["BeforeToolCall"]
command = "./handler.sh"
"#,
            "#!/bin/sh\nexit 2\n",
        );

        let config = Config {
            core: crate::config::CoreConfig {
                workspace: temp.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let manager = HookManager::discover(&config);
        for _ in 0..3 {
            let _ = manager.before_tool_call(sample_before_payload()).await;
        }
        let hook = manager.before_tool_call.first().expect("hook");
        let state = hook.state.lock().await;
        assert!(state.disabled_until.is_some());
    }

    #[tokio::test]
    async fn status_and_inspect_surface_discovered_hooks() {
        let temp = TempDir::new().expect("tempdir");
        let hooks_root = temp.path().join(".rove").join("hooks");
        write_hook(
            &hooks_root,
            "audit",
            r#"
name = "audit"
description = "Audit tool calls"
events = ["BeforeToolCall", "AfterToolCall"]
command = "./handler.sh"
timeout = 7
"#,
            "#!/bin/sh\nprintf '{\"action\":\"continue\"}'\n",
        );

        let config = Config {
            core: crate::config::CoreConfig {
                workspace: temp.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };

        let manager = HookManager::discover(&config);
        let status = manager.status().await;
        assert_eq!(status.hooks.len(), 1);
        assert_eq!(status.hooks[0].name, "audit");
        assert_eq!(
            status.hooks[0].events,
            vec!["BeforeToolCall".to_string(), "AfterToolCall".to_string()]
        );
        assert_eq!(status.hooks[0].timeout_secs, 7);

        let inspect = manager.inspect("audit").await.expect("inspect hook");
        assert_eq!(inspect.name, "audit");
        assert_eq!(inspect.description.as_deref(), Some("Audit tool calls"));
    }
}
