//! Browser backend trait and standard tool names.
//!
//! This module defines the contract that pluggable browser backends must
//! implement to participate in Rove's `[browser]` config section, profile
//! switching, and `rove browser status`.
//!
//! Plugins that implement only [`CoreTool`](crate::CoreTool) (without
//! `BrowserBackend`) work fine as regular native tools but do not
//! participate in the browser profile/lifecycle system.

use async_trait::async_trait;

use crate::errors::EngineError;

/// Standard tool names for browser backends.
///
/// Plugins SHOULD use these names for interoperability.
/// Using different names is allowed but breaks the standard toolset
/// convention and will not be recognized by `[browser]` config integration.
pub mod tool_names {
    pub const BROWSE_URL: &str = "browse_url";
    pub const READ_PAGE_TEXT: &str = "read_page_text";
    pub const CLICK_ELEMENT: &str = "click_element";
    pub const FILL_FORM_FIELD: &str = "fill_form_field";
}

/// Trait for pluggable browser backends.
///
/// Implement this trait to integrate with Rove's `[browser]` config
/// section, profile switching, and `rove browser status`.
///
/// # Connection Lifecycle
///
/// The plugin owns its connection lifecycle. The engine calls trait
/// methods and the backend is responsible for connecting lazily,
/// reconnecting on failure, and cleaning up on drop.
///
/// # Example
///
/// ```ignore
/// use sdk::browser::BrowserBackend;
/// use sdk::errors::EngineError;
///
/// struct MyCdpBackend { /* ... */ }
///
/// #[async_trait::async_trait]
/// impl BrowserBackend for MyCdpBackend {
///     async fn navigate(&mut self, url: &str) -> Result<String, EngineError> {
///         // connect to Chrome, navigate, return title
///         Ok(format!("Navigated to {}", url))
///     }
///     async fn page_text(&mut self) -> Result<String, EngineError> {
///         Ok("page content".to_string())
///     }
///     async fn click(&mut self, selector: &str) -> Result<String, EngineError> {
///         Ok(format!("Clicked {}", selector))
///     }
///     async fn fill_field(&mut self, selector: &str, value: &str) -> Result<String, EngineError> {
///         Ok(format!("Filled {} with {}", selector, value))
///     }
///     fn backend_name(&self) -> &str { "my-cdp" }
///     fn is_connected(&self) -> bool { false }
/// }
/// ```
#[async_trait]
pub trait BrowserBackend: Send + Sync {
    /// Navigate to a URL and return a summary (e.g. page title).
    async fn navigate(&mut self, url: &str) -> Result<String, EngineError>;

    /// Return the visible text of the current page.
    async fn page_text(&mut self) -> Result<String, EngineError>;

    /// Click the first element matching a CSS selector.
    async fn click(&mut self, selector: &str) -> Result<String, EngineError>;

    /// Fill a form field matching a CSS selector with a value.
    async fn fill_field(&mut self, selector: &str, value: &str) -> Result<String, EngineError>;

    /// Human-readable name of this backend (e.g. "Chrome CDP", "Browsh").
    fn backend_name(&self) -> &str;

    /// Whether this backend is currently connected.
    fn is_connected(&self) -> bool;
}
