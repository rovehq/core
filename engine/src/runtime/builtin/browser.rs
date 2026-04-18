//! Browser tool — CDP-backed browser automation.
//!
//! Provides four built-in tools:
//!   - `browse_url`      — navigate to a URL and return title
//!   - `read_page_text`  — get visible text content of the current page
//!   - `click_element`   — click a DOM element by CSS selector
//!   - `fill_form_field` — fill an input field by CSS selector
//!
//! Three profile modes:
//!   - `ManagedLocal`   — Rove spawns Chrome headlessly and owns the process
//!   - `AttachExisting` — connect to an already-running Chrome via its CDP port
//!   - `RemoteCdp`      — connect to a remote CDP WebSocket endpoint

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, StreamExt};
use tokio::process::Child;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use tracing::{debug, info, warn};

use sdk::browser::BrowserBackend;
use sdk::errors::EngineError;

use crate::config::{BrowserProfileConfig, BrowserProfileMode};

// ---------------------------------------------------------------------------
// CDP WebSocket session
// ---------------------------------------------------------------------------

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct CdpSession {
    ws: WsStream,
    next_id: u64,
}

impl CdpSession {
    /// Send a CDP method call and wait for its matching response.
    ///
    /// Events (messages without an `id` field) and responses for other
    /// in-flight ids are skipped — this is safe because the agent tool loop
    /// is sequential and only one CDP call is in-flight at a time.
    async fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;

        let req = serde_json::json!({"id": id, "method": method, "params": params});
        self.ws
            .send(Message::Text(req.to_string().into()))
            .await
            .context("Failed to send CDP command")?;

        loop {
            match tokio::time::timeout(Duration::from_secs(15), self.ws.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    let parsed: serde_json::Value =
                        serde_json::from_str(&text).context("CDP response is not valid JSON")?;
                    // Only handle responses that belong to our request id
                    if parsed.get("id").and_then(|v| v.as_u64()) == Some(id) {
                        if let Some(error) = parsed.get("error") {
                            return Err(anyhow!("CDP error for {}: {}", method, error));
                        }
                        return Ok(parsed
                            .get("result")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null));
                    }
                    // Skip events (no id) and responses for other ids
                }
                Ok(Some(Ok(_))) => {} // Binary / ping / pong — skip
                Ok(Some(Err(e))) => return Err(anyhow!("CDP WebSocket error: {}", e)),
                Ok(None) => return Err(anyhow!("CDP WebSocket closed unexpectedly")),
                Err(_) => {
                    return Err(anyhow!(
                        "CDP command '{}' timed out after 15 seconds",
                        method
                    ))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// BrowserTool
// ---------------------------------------------------------------------------

pub struct BrowserTool {
    profile: BrowserProfileConfig,
    /// Live CDP session; `None` until first use.
    session: Option<CdpSession>,
    /// Chrome child process kept alive in ManagedLocal mode.
    chrome_process: Option<Child>,
}

impl BrowserTool {
    pub fn new(profile: BrowserProfileConfig) -> Self {
        Self {
            profile,
            session: None,
            chrome_process: None,
        }
    }

    // ------------------------------------------------------------------
    // Connection management
    // ------------------------------------------------------------------

    /// Connect (or reuse an existing connection) to Chrome via CDP.
    async fn ensure_connected(&mut self) -> Result<()> {
        if self.session.is_some() {
            return Ok(());
        }

        let endpoint = self.resolve_endpoint().await?;
        let ws_url = if endpoint.starts_with("ws://") || endpoint.starts_with("wss://") {
            // Caller provided a direct WebSocket URL
            endpoint
        } else {
            // HTTP base URL — find or create a page target
            get_page_ws_url(&endpoint)
                .await
                .with_context(|| format!("Failed to get page WebSocket URL from {}", endpoint))?
        };

        debug!(ws_url = %ws_url, "connecting to CDP");
        let (ws, _) = connect_async(&ws_url)
            .await
            .with_context(|| format!("Failed to connect CDP WebSocket at {}", ws_url))?;

        let mut session = CdpSession { ws, next_id: 1 };
        // Enable Page and Runtime domains
        session
            .call("Page.enable", serde_json::json!({}))
            .await
            .context("Page.enable failed")?;
        session
            .call("Runtime.enable", serde_json::json!({}))
            .await
            .context("Runtime.enable failed")?;

        self.session = Some(session);
        Ok(())
    }

    /// Determine the CDP endpoint (HTTP base or ws:// URL) for this profile.
    async fn resolve_endpoint(&mut self) -> Result<String> {
        match self.profile.mode {
            BrowserProfileMode::ManagedLocal => self.launch_managed_chrome().await,
            BrowserProfileMode::AttachExisting => Ok(self
                .profile
                .cdp_url
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:9222".to_string())),
            BrowserProfileMode::RemoteCdp => self
                .profile
                .cdp_url
                .clone()
                .ok_or_else(|| anyhow!("RemoteCdp profile requires a cdp_url to be configured")),
        }
    }

    /// Spawn Chrome with a remote-debugging port and wait until it is ready.
    async fn launch_managed_chrome(&mut self) -> Result<String> {
        let binary = resolve_chrome_binary(self.profile.browser.as_deref()).ok_or_else(|| {
            anyhow!(
                "Chrome/Chromium not found. Install Google Chrome or set the browser executable \
                 path in the profile config."
            )
        })?;

        let port: u16 = 9222;
        let user_data_dir = self.profile.user_data_dir.clone().unwrap_or_else(|| {
            std::env::temp_dir()
                .join("rove-browser")
                .to_string_lossy()
                .into_owned()
        });

        let mut args = vec![
            format!("--remote-debugging-port={}", port),
            "--headless=new".to_string(),
            "--no-sandbox".to_string(),
            "--disable-gpu".to_string(),
            "--disable-dev-shm-usage".to_string(),
            format!("--user-data-dir={}", user_data_dir),
        ];
        if let Some(url) = &self.profile.startup_url {
            args.push(url.clone());
        }

        info!(binary = %binary.display(), args = ?args, "launching managed Chrome");

        let child = tokio::process::Command::new(&binary)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to spawn Chrome at {}", binary.display()))?;
        self.chrome_process = Some(child);

        // Poll /json/version until Chrome is accepting requests (up to 5 s)
        let base_url = format!("http://127.0.0.1:{}", port);
        let client = reqwest::Client::new();
        for _ in 0..25 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if client
                .get(format!("{}/json/version", base_url))
                .send()
                .await
                .is_ok()
            {
                info!("Chrome CDP ready at {}", base_url);
                return Ok(base_url);
            }
        }

        Err(anyhow!(
            "Chrome did not respond on port {} within 5 seconds",
            port
        ))
    }

    // ------------------------------------------------------------------
    // Public tool methods
    // ------------------------------------------------------------------

    /// Navigate to `url` and return the page title.
    pub async fn navigate(&mut self, url: &str) -> Result<String> {
        self.ensure_connected().await?;
        let session = self.session.as_mut().unwrap();

        session
            .call("Page.navigate", serde_json::json!({"url": url}))
            .await
            .with_context(|| format!("Page.navigate failed for {}", url))?;

        // Poll document.readyState until "complete" (up to 4 s)
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let state = session
                .call(
                    "Runtime.evaluate",
                    serde_json::json!({"expression": "document.readyState", "returnByValue": true}),
                )
                .await
                .unwrap_or_default();
            if ready_state_value(&state) == Some("complete") {
                break;
            }
        }

        // Read title
        let title_res = session
            .call(
                "Runtime.evaluate",
                serde_json::json!({"expression": "document.title", "returnByValue": true}),
            )
            .await
            .unwrap_or_default();
        let title = extract_string_value(&title_res)
            .unwrap_or("(no title)")
            .to_string();

        debug!(url = %url, title = %title, "navigation complete");
        Ok(format!("Navigated to: {url}\nTitle: {title}"))
    }

    /// Return the visible text of the current page (up to 8 000 chars).
    pub async fn page_text(&mut self) -> Result<String> {
        self.ensure_connected().await?;
        let session = self.session.as_mut().unwrap();

        let res = session
            .call(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "document.body ? document.body.innerText : ''",
                    "returnByValue": true
                }),
            )
            .await?;

        let text = extract_string_value(&res).unwrap_or("").to_string();
        if text.is_empty() {
            return Ok("Page has no visible text content.".to_string());
        }
        const MAX: usize = 8_000;
        if text.len() > MAX {
            Ok(format!(
                "{}\n[truncated — {} chars total]",
                &text[..MAX],
                text.len()
            ))
        } else {
            Ok(text)
        }
    }

    /// Click the first DOM element matching `selector`.
    pub async fn click(&mut self, selector: &str) -> Result<String> {
        self.ensure_connected().await?;
        let session = self.session.as_mut().unwrap();

        // Build a JS snippet that returns a status string.
        // We JSON-encode the selector to avoid injection through backtick/backslash.
        let selector_json = serde_json::to_string(selector).unwrap_or_default();
        let expr = format!(
            r#"(function(){{
                const sel = {selector_json};
                const el = document.querySelector(sel);
                if (!el) return 'ERROR: no element matches selector: ' + sel;
                el.click();
                const label = el.textContent.trim().slice(0, 60) || el.tagName;
                return 'Clicked: ' + label;
            }})()"#
        );

        let res = session
            .call(
                "Runtime.evaluate",
                serde_json::json!({"expression": expr, "returnByValue": true}),
            )
            .await?;

        Ok(extract_string_value(&res)
            .unwrap_or("Click executed")
            .to_string())
    }

    /// Set `value` on the first form field matching `selector`.
    pub async fn fill_field(&mut self, selector: &str, value: &str) -> Result<String> {
        self.ensure_connected().await?;
        let session = self.session.as_mut().unwrap();

        let selector_json = serde_json::to_string(selector).unwrap_or_default();
        let value_json = serde_json::to_string(value).unwrap_or_default();
        let expr = format!(
            r#"(function(){{
                const sel = {selector_json};
                const val = {value_json};
                const el = document.querySelector(sel);
                if (!el) return 'ERROR: no element matches selector: ' + sel;
                el.focus();
                el.value = val;
                el.dispatchEvent(new Event('input', {{bubbles: true}}));
                el.dispatchEvent(new Event('change', {{bubbles: true}}));
                return 'Filled ' + (el.name || el.id || el.tagName);
            }})()"#
        );

        let res = session
            .call(
                "Runtime.evaluate",
                serde_json::json!({"expression": expr, "returnByValue": true}),
            )
            .await?;

        Ok(extract_string_value(&res)
            .unwrap_or("Field filled")
            .to_string())
    }
}

impl Drop for BrowserTool {
    fn drop(&mut self) {
        if let Some(mut child) = self.chrome_process.take() {
            // start_kill sends the signal without awaiting — safe to call in Drop
            if let Err(e) = child.start_kill() {
                warn!(error = %e, "Failed to kill managed Chrome on drop");
            }
        }
    }
}

#[async_trait::async_trait]
impl BrowserBackend for BrowserTool {
    async fn navigate(&mut self, url: &str) -> Result<String, EngineError> {
        self.navigate(url)
            .await
            .map_err(|e| EngineError::ToolError(e.to_string()))
    }

    async fn page_text(&mut self) -> Result<String, EngineError> {
        self.page_text()
            .await
            .map_err(|e| EngineError::ToolError(e.to_string()))
    }

    async fn click(&mut self, selector: &str) -> Result<String, EngineError> {
        self.click(selector)
            .await
            .map_err(|e| EngineError::ToolError(e.to_string()))
    }

    async fn fill_field(&mut self, selector: &str, value: &str) -> Result<String, EngineError> {
        self.fill_field(selector, value)
            .await
            .map_err(|e| EngineError::ToolError(e.to_string()))
    }

    fn backend_name(&self) -> &str {
        "Chrome CDP"
    }

    fn is_connected(&self) -> bool {
        self.session.is_some()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the WebSocket debugger URL of an existing (or newly created) page
/// target in the Chrome instance listening at `http_base`.
async fn get_page_ws_url(http_base: &str) -> Result<String> {
    // Normalise: strip trailing slash, ensure http scheme
    let base = http_base
        .replace("ws://", "http://")
        .replace("wss://", "https://")
        .trim_end_matches('/')
        .to_string();

    let client = reqwest::Client::new();

    // Try /json/list first — pick the first "page" target
    let list_url = format!("{}/json/list", base);
    if let Ok(resp) = client.get(&list_url).send().await {
        if let Ok(targets) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = targets.as_array() {
                for target in arr {
                    if target.get("type").and_then(|v| v.as_str()) == Some("page") {
                        if let Some(ws) =
                            target.get("webSocketDebuggerUrl").and_then(|v| v.as_str())
                        {
                            return Ok(ws.to_string());
                        }
                    }
                }
            }
        }
    }

    // No existing page target — create a new blank tab
    let new_url = format!("{}/json/new", base);
    let tab: serde_json::Value = client
        .get(&new_url)
        .send()
        .await
        .with_context(|| format!("GET {} failed — is Chrome running?", new_url))?
        .json()
        .await
        .context("Invalid /json/new response")?;

    tab.get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("webSocketDebuggerUrl missing in /json/new response"))
}

/// Find a Chrome or Chromium binary.
/// `override_path` is taken from the profile `browser` field when set.
fn resolve_chrome_binary(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = override_path {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let candidates = [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        ];
        for path in &candidates {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let candidates = [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium-browser",
            "/usr/bin/chromium",
            "/usr/local/bin/google-chrome",
            "/snap/bin/chromium",
        ];
        for path in &candidates {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ];
        for path in &candidates {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}

/// Extract `result.value` as `&str` from a `Runtime.evaluate` response.
fn extract_string_value(res: &serde_json::Value) -> Option<&str> {
    res.get("result")?.get("value")?.as_str()
}

/// Extract the `readyState` string from a `Runtime.evaluate` response.
fn ready_state_value(res: &serde_json::Value) -> Option<&str> {
    extract_string_value(res)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BrowserProfileConfig;

    #[test]
    fn browser_tool_constructs_without_panic() {
        let profile = BrowserProfileConfig {
            id: "test".to_string(),
            name: "Test".to_string(),
            mode: BrowserProfileMode::AttachExisting,
            cdp_url: Some("http://127.0.0.1:9222".to_string()),
            ..Default::default()
        };
        let tool = BrowserTool::new(profile);
        assert!(tool.session.is_none());
        assert!(tool.chrome_process.is_none());
    }

    #[test]
    fn resolve_chrome_binary_does_not_return_nonexistent_override() {
        // A nonexistent override path must not be returned; the function either
        // falls back to a system binary or returns None.
        let nonexistent = "/nonexistent/chrome/path/that/cannot/exist";
        let result = resolve_chrome_binary(Some(nonexistent));
        if let Some(path) = result {
            assert_ne!(path, std::path::PathBuf::from(nonexistent));
        }
    }

    #[test]
    fn extract_string_value_parses_evaluate_response() {
        let res = serde_json::json!({
            "result": {"type": "string", "value": "complete"}
        });
        assert_eq!(extract_string_value(&res), Some("complete"));
    }

    #[test]
    fn extract_string_value_returns_none_for_non_string() {
        let res = serde_json::json!({"result": {"type": "number", "value": 42}});
        // value is a number, as_str() returns None
        assert!(extract_string_value(&res).is_none());
    }
}
