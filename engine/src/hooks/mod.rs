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
    #[serde(rename = "BeforeAgentStart")]
    BeforeAgentStart,
    #[serde(rename = "MessageReceived")]
    MessageReceived,
    #[serde(rename = "MessageSending")]
    MessageSending,
    #[serde(rename = "SessionStart")]
    SessionStart,
    #[serde(rename = "SessionEnd")]
    SessionEnd,
}

impl HookEvent {
    fn as_str(self) -> &'static str {
        match self {
            HookEvent::BeforeToolCall => "BeforeToolCall",
            HookEvent::AfterToolCall => "AfterToolCall",
            HookEvent::BeforeAgentStart => "BeforeAgentStart",
            HookEvent::MessageReceived => "MessageReceived",
            HookEvent::MessageSending => "MessageSending",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::SessionEnd => "SessionEnd",
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
    before_agent_start: Vec<Arc<HookHandle>>,
    message_received: Vec<Arc<HookHandle>>,
    message_sending: Vec<Arc<HookHandle>>,
    session_start: Vec<Arc<HookHandle>>,
    session_end: Vec<Arc<HookHandle>>,
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

#[derive(Debug, Clone, Serialize)]
pub struct BeforeAgentStartPayload {
    pub event: &'static str,
    pub task_id: String,
    pub input: String,
    pub task_source: String,
    pub workspace: String,
    pub session_id: Option<String>,
    pub run_mode: String,
    pub run_isolation: String,
    pub execution_profile: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageReceivedPayload {
    pub event: &'static str,
    pub task_id: String,
    pub input: String,
    pub task_source: String,
    pub workspace: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageSendingPayload {
    pub event: &'static str,
    pub task_id: String,
    pub output: String,
    pub task_source: String,
    pub workspace: String,
    pub session_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStartPayload {
    pub event: &'static str,
    pub session_id: String,
    pub client_label: Option<String>,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub workspace: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionEndPayload {
    pub event: &'static str,
    pub session_id: String,
    pub reason: String,
    pub client_label: Option<String>,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub workspace: String,
}

#[derive(Debug)]
pub struct BeforeToolCallOutcome {
    pub args: Value,
    pub modified_by: Vec<String>,
}

#[derive(Debug)]
pub struct TextHookOutcome {
    pub text: String,
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
                manager.after_tool_call.push(Arc::clone(&handle));
            }
            if handle
                .definition
                .events
                .contains(&HookEvent::BeforeAgentStart)
            {
                manager.before_agent_start.push(Arc::clone(&handle));
            }
            if handle
                .definition
                .events
                .contains(&HookEvent::MessageReceived)
            {
                manager.message_received.push(Arc::clone(&handle));
            }
            if handle
                .definition
                .events
                .contains(&HookEvent::MessageSending)
            {
                manager.message_sending.push(Arc::clone(&handle));
            }
            if handle.definition.events.contains(&HookEvent::SessionStart) {
                manager.session_start.push(Arc::clone(&handle));
            }
            if handle.definition.events.contains(&HookEvent::SessionEnd) {
                manager.session_end.push(handle);
            }
        }
        manager
            .before_tool_call
            .sort_by(|left, right| left.definition.name.cmp(&right.definition.name));
        manager
            .after_tool_call
            .sort_by(|left, right| left.definition.name.cmp(&right.definition.name));
        manager
            .before_agent_start
            .sort_by(|left, right| left.definition.name.cmp(&right.definition.name));
        manager
            .message_received
            .sort_by(|left, right| left.definition.name.cmp(&right.definition.name));
        manager
            .message_sending
            .sort_by(|left, right| left.definition.name.cmp(&right.definition.name));
        manager
            .session_start
            .sort_by(|left, right| left.definition.name.cmp(&right.definition.name));
        manager
            .session_end
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
        self.run_read_only_hooks(&self.after_tool_call, payload, "AfterToolCall hook failed")
            .await;
    }

    pub async fn before_agent_start(
        &self,
        payload: BeforeAgentStartPayload,
    ) -> Result<TextHookOutcome, EngineError> {
        self.run_mutating_text_hooks(&self.before_agent_start, payload, "input")
            .await
    }

    pub async fn message_received(
        &self,
        payload: MessageReceivedPayload,
    ) -> Result<TextHookOutcome, EngineError> {
        self.run_mutating_text_hooks(&self.message_received, payload, "input")
            .await
    }

    pub async fn message_sending(&self, payload: MessageSendingPayload) {
        self.run_read_only_hooks(&self.message_sending, payload, "MessageSending hook failed")
            .await;
    }

    pub async fn session_start(&self, payload: SessionStartPayload) {
        self.run_read_only_hooks(&self.session_start, payload, "SessionStart hook failed")
            .await;
    }

    pub async fn session_end(&self, payload: SessionEndPayload) {
        self.run_read_only_hooks(&self.session_end, payload, "SessionEnd hook failed")
            .await;
    }

    pub async fn status(&self) -> HookStatus {
        let mut hooks = Vec::new();
        for handle in self
            .before_tool_call
            .iter()
            .chain(self.after_tool_call.iter())
            .chain(self.before_agent_start.iter())
            .chain(self.message_received.iter())
            .chain(self.message_sending.iter())
            .chain(self.session_start.iter())
            .chain(self.session_end.iter())
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
            .chain(self.before_agent_start.iter())
            .chain(self.message_received.iter())
            .chain(self.message_sending.iter())
            .chain(self.session_start.iter())
            .chain(self.session_end.iter())
        {
            if handle.definition.name == name {
                return Some(handle.summary().await);
            }
        }
        None
    }

    async fn run_mutating_text_hooks<T: Serialize + Clone>(
        &self,
        hooks: &[Arc<HookHandle>],
        payload: T,
        field_name: &str,
    ) -> Result<TextHookOutcome, EngineError> {
        let mut text = extract_text_field(&payload, field_name).unwrap_or_default();
        let mut modified_by = Vec::new();

        for hook in hooks {
            let next_payload = rewrite_text_field(&payload, field_name, &text);
            match hook.run_before_text_hook(&next_payload, field_name).await {
                Ok(BeforeHookResult::Continue) => {}
                Ok(BeforeHookResult::Modify(updated)) => {
                    text = value_to_text(updated, field_name)
                        .map_err(|error| EngineError::ToolError(error.to_string()))?;
                    modified_by.push(hook.definition.name.clone());
                }
                Ok(BeforeHookResult::Block(reason)) => {
                    return Err(EngineError::ToolError(format!(
                        "hook '{}' blocked lifecycle event: {}",
                        hook.definition.name, reason
                    )));
                }
                Err(error) => {
                    warn!(
                        hook = %hook.definition.name,
                        error = %error,
                        "Lifecycle mutating hook failed"
                    );
                }
            }
        }

        Ok(TextHookOutcome { text, modified_by })
    }

    async fn run_read_only_hooks<T: Serialize + Clone + Send + Sync + 'static>(
        &self,
        hooks: &[Arc<HookHandle>],
        payload: T,
        error_message: &str,
    ) {
        let mut jobs = JoinSet::new();
        for hook in hooks {
            let hook = Arc::clone(hook);
            let payload = payload.clone();
            let error_message = error_message.to_string();
            jobs.spawn(async move {
                if let Err(error) = hook.run_read_only_hook(payload).await {
                    warn!(hook = %hook.definition.name, error = %error, "{error_message}");
                }
            });
        }

        while jobs.join_next().await.is_some() {}
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

    async fn run_before_text_hook<T: Serialize>(
        &self,
        payload: &T,
        field_name: &str,
    ) -> anyhow::Result<BeforeHookResult> {
        let output = self.run_json_hook(payload).await?;
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
                let text = value_to_text(data, field_name)?;
                Ok(BeforeHookResult::Modify(Value::String(text)))
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

    async fn run_read_only_hook<T: Serialize + Sync>(&self, payload: T) -> anyhow::Result<()> {
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

fn extract_text_field<T: Serialize>(payload: &T, field_name: &str) -> Option<String> {
    serde_json::to_value(payload)
        .ok()
        .and_then(|value| value.get(field_name).cloned())
        .and_then(|value| value.as_str().map(|value| value.to_string()))
}

fn rewrite_text_field<T: Serialize>(payload: &T, field_name: &str, text: &str) -> Value {
    let mut value = serde_json::to_value(payload).unwrap_or(Value::Null);
    if let Value::Object(ref mut object) = value {
        object.insert(field_name.to_string(), Value::String(text.to_string()));
    }
    value
}

fn value_to_text(value: Value, field_name: &str) -> anyhow::Result<String> {
    match value {
        Value::String(text) => Ok(text),
        Value::Object(mut object) => match object.remove(field_name) {
            Some(Value::String(text)) => Ok(text),
            Some(other) => Err(anyhow::anyhow!(
                "hook returned non-string '{}' field: {}",
                field_name,
                other
            )),
            None => Err(anyhow::anyhow!(
                "hook returned modify payload without '{}' field",
                field_name
            )),
        },
        other => Err(anyhow::anyhow!(
            "hook returned unsupported modify payload: {}",
            other
        )),
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

    fn sample_message_received_payload() -> MessageReceivedPayload {
        MessageReceivedPayload {
            event: "MessageReceived",
            task_id: "task-1".to_string(),
            input: "original input".to_string(),
            task_source: "cli".to_string(),
            workspace: "/tmp/workspace".to_string(),
            session_id: Some("session-1".to_string()),
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
    async fn message_received_can_modify_input() {
        let temp = TempDir::new().expect("tempdir");
        let hooks_root = temp.path().join(".rove").join("hooks");
        write_hook(
            &hooks_root,
            "rewrite-message",
            r#"
name = "rewrite-message"
events = ["MessageReceived"]
command = "./handler.sh"
"#,
            "#!/bin/sh\nprintf '{\"action\":\"modify\",\"data\":{\"input\":\"rewritten input\"}}'\n",
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
            .message_received(sample_message_received_payload())
            .await
            .expect("message received result");
        assert_eq!(result.text, "rewritten input");
        assert_eq!(result.modified_by, vec!["rewrite-message".to_string()]);
    }

    #[tokio::test]
    async fn session_start_runs_read_only_hook() {
        let temp = TempDir::new().expect("tempdir");
        let hooks_root = temp.path().join(".rove").join("hooks");
        let output_path = temp.path().join("session-start.json");
        write_hook(
            &hooks_root,
            "session-start-audit",
            &format!(
                r#"
name = "session-start-audit"
events = ["SessionStart"]
command = "./handler.sh"
"#
            ),
            &format!(
                "#!/bin/sh\ncat > \"{}\"\nprintf '{{\"action\":\"continue\"}}'\n",
                output_path.display()
            ),
        );

        let config = Config {
            core: crate::config::CoreConfig {
                workspace: temp.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let manager = HookManager::discover(&config);
        manager
            .session_start(SessionStartPayload {
                event: "SessionStart",
                session_id: "session-1".to_string(),
                client_label: Some("webui".to_string()),
                origin: Some("http://localhost:3000".to_string()),
                user_agent: Some("test-agent".to_string()),
                workspace: temp.path().display().to_string(),
            })
            .await;

        let payload = std::fs::read_to_string(&output_path).expect("session-start payload");
        assert!(payload.contains("\"event\":\"SessionStart\""));
        assert!(payload.contains("\"session_id\":\"session-1\""));
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
events = ["BeforeToolCall", "AfterToolCall", "MessageSending", "SessionEnd"]
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
            vec![
                "BeforeToolCall".to_string(),
                "AfterToolCall".to_string(),
                "MessageSending".to_string(),
                "SessionEnd".to_string(),
            ]
        );
        assert_eq!(status.hooks[0].timeout_secs, 7);

        let inspect = manager.inspect("audit").await.expect("inspect hook");
        assert_eq!(inspect.name, "audit");
        assert_eq!(inspect.description.as_deref(), Some("Audit tool calls"));
    }
}
