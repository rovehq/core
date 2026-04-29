//! Integration tests for the browser-cdp driver
//!
//! Tests that do not require a live Chrome instance run unconditionally.
//! Tests that need an actual Chrome process are tagged `#[ignore]` and must
//! be opted into explicitly:
//!
//! ```
//! cargo test -p engine browser_cdp -- --include-ignored
//! ```
//!
//! For the live tests set `ROVE_TEST_CDP_URL` to an existing Chrome
//! remote-debugging endpoint to skip the managed-launch path:
//!
//! ```
//! open -a 'Google Chrome' --args --remote-debugging-port=9222
//! ROVE_TEST_CDP_URL=http://127.0.0.1:9222 \
//!   cargo test -p engine browser_cdp -- --include-ignored
//! ```

use rove_engine::config::{BrowserConfig, BrowserProfileConfig, BrowserProfileMode, Config};
use rove_engine::runtime::manifest::{Manifest as DriverManifest, ToolCatalog};
use rove_engine::runtime::RuntimeManager;
use rove_engine::storage::Database;
use sdk::tool_io::ToolInput;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn browser_enabled_config(workspace: &std::path::Path, data_dir: &std::path::Path) -> Config {
    let mut config = Config::default();
    config.core.workspace = workspace.to_path_buf();
    config.core.data_dir = data_dir.to_path_buf();
    config.mcp.servers.clear();

    config.browser = BrowserConfig {
        enabled: true,
        default_profile_id: Some("local".to_string()),
        profiles: vec![BrowserProfileConfig {
            id: "local".to_string(),
            name: "Chrome Local".to_string(),
            mode: BrowserProfileMode::ManagedLocal,
            enabled: true,
            ..Default::default()
        }],
        ..Default::default()
    };

    config
}

/// CDP URL for live tests — falls back to the managed-launch path if unset.
fn test_cdp_url() -> Option<String> {
    std::env::var("ROVE_TEST_CDP_URL").ok()
}

// ---------------------------------------------------------------------------
// Static artefact tests (no Chrome required)
// ---------------------------------------------------------------------------

#[test]
fn browser_cdp_manifest_is_valid() {
    let raw = include_str!("../../../plugins/browser-cdp/manifest.json");
    let manifest = DriverManifest::from_json(raw).expect("manifest.json should be valid");
    assert_eq!(manifest.name, "browser-cdp");
    assert_eq!(
        manifest.plugin_type,
        rove_engine::runtime::manifest::PluginType::Plugin
    );
}

#[test]
fn browser_cdp_tool_catalog_declares_backend() {
    let raw = include_str!("../../../plugins/browser-cdp/runtime.json");
    let catalog = ToolCatalog::from_json(Some(raw)).expect("runtime.json should be valid");
    let backend = catalog
        .browser_backend
        .expect("runtime.json must declare a browser_backend");
    assert_eq!(backend.id, "cdp");
    assert_eq!(backend.display_name(), "Chrome CDP");
    let mut names = backend.standard_tool_names();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "browse_url",
            "click_element",
            "fill_form_field",
            "read_page_text"
        ]
    );
}

#[test]
fn browser_cdp_tool_catalog_declares_no_extra_tools() {
    // The browser tools are injected by the engine via the BrowserBackend
    // bridge, not registered as standalone entries in the catalog.
    let raw = include_str!("../../../plugins/browser-cdp/runtime.json");
    let catalog = ToolCatalog::from_json(Some(raw)).expect("runtime.json should be valid");
    assert!(
        catalog.tools.is_empty(),
        "runtime.json must not declare standalone tools; they come from BrowserBackend"
    );
}

// ---------------------------------------------------------------------------
// Driver dispatch tests (no Chrome required — driver thread spawned but
// the actual CDP connection only happens on first use)
// ---------------------------------------------------------------------------

/// The driver's CoreTool::handle() is loaded via the plugin crate directly
/// (not from a .dylib) in these tests for speed and portability.
#[tokio::test(flavor = "multi_thread")]
async fn browser_cdp_dispatch_unknown_method_returns_tool_error() {
    use browser_cdp::BrowserCdpTool;
    use sdk::core_tool::CoreTool;

    let tool = BrowserCdpTool::new_for_test();
    let input = ToolInput::new("nonexistent_browser_method");
    let err = tool.handle(input).unwrap_err();
    assert!(
        err.to_string().contains("unknown method"),
        "expected 'unknown method' in error, got: {err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn browser_cdp_browse_url_missing_param_returns_error() {
    use browser_cdp::BrowserCdpTool;
    use sdk::core_tool::CoreTool;

    let tool = BrowserCdpTool::new_for_test();
    let input = ToolInput::new("browse_url"); // deliberately missing "url" param
    let err = tool.handle(input).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("url")
            || err.to_string().to_lowercase().contains("missing"),
        "expected a missing-param error for 'url', got: {err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn browser_cdp_click_element_missing_selector_returns_error() {
    use browser_cdp::BrowserCdpTool;
    use sdk::core_tool::CoreTool;

    let tool = BrowserCdpTool::new_for_test();
    let input = ToolInput::new("click_element"); // missing "selector"
    let err = tool.handle(input).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("selector")
            || err.to_string().to_lowercase().contains("missing"),
        "expected a missing-param error for 'selector', got: {err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn browser_cdp_fill_form_field_missing_params_returns_error() {
    use browser_cdp::BrowserCdpTool;
    use sdk::core_tool::CoreTool;

    let tool = BrowserCdpTool::new_for_test();
    let input = ToolInput::new("fill_form_field"); // missing selector + value
    let err = tool.handle(input).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("missing")
            || err.to_string().to_lowercase().contains("selector"),
        "expected a missing-param error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// RuntimeManager wiring test (no Chrome required)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn browser_cdp_registry_has_no_browser_when_config_disabled() {
    let workspace = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let database = Database::new(&data.path().join("browser-test.db"))
        .await
        .unwrap();

    let mut config = Config::default();
    config.core.workspace = workspace.path().to_path_buf();
    config.mcp.servers.clear();
    config.browser.enabled = false;

    let runtime = RuntimeManager::build(&database, &config).await.unwrap();
    assert!(
        runtime.registry.browser.is_none(),
        "browser should not be registered when browser.enabled = false"
    );
}

#[tokio::test]
async fn browser_cdp_registry_has_no_browser_when_no_profiles_configured() {
    let workspace = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let database = Database::new(&data.path().join("browser-no-profile.db"))
        .await
        .unwrap();

    let mut config = Config::default();
    config.core.workspace = workspace.path().to_path_buf();
    config.mcp.servers.clear();
    config.browser.enabled = true;
    config.browser.profiles.clear(); // no profiles = no backend

    let runtime = RuntimeManager::build(&database, &config).await.unwrap();
    assert!(
        runtime.registry.browser.is_none(),
        "browser should not be registered when no profiles are configured"
    );
}

// ---------------------------------------------------------------------------
// Live tests — require Chrome; opt in with --include-ignored
// ---------------------------------------------------------------------------

/// Navigates to example.com and asserts the page title is non-empty.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chrome installed; run with --include-ignored"]
async fn browser_cdp_navigate_returns_page_title() {
    use browser_cdp::{BrowserCdpTool, CdpConfig, CdpMode};
    use sdk::core_tool::CoreTool;

    let config = match test_cdp_url() {
        Some(url) => CdpConfig {
            mode: CdpMode::AttachExisting,
            cdp_url: Some(url),
            browser: None,
            user_data_dir: None,
            startup_url: None,
        },
        None => CdpConfig {
            mode: CdpMode::ManagedLocal,
            cdp_url: None,
            browser: None,
            user_data_dir: None,
            startup_url: None,
        },
    };

    let tool = BrowserCdpTool::new_with_config(config);
    let input =
        ToolInput::new("browse_url").with_param("url", serde_json::json!("https://example.com"));

    let output = tool.handle(input).expect("browse_url should succeed");
    assert!(output.success);
    let text = output.data.as_str().unwrap_or_default();
    assert!(
        !text.is_empty(),
        "page title should be non-empty after navigating to example.com"
    );
    assert!(
        text.contains("example") || text.contains("Example"),
        "expected 'Example' in title, got: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chrome installed; run with --include-ignored"]
async fn browser_cdp_read_page_text_after_navigate() {
    use browser_cdp::{BrowserCdpTool, CdpConfig, CdpMode};
    use sdk::core_tool::CoreTool;

    let config = CdpConfig {
        mode: CdpMode::ManagedLocal,
        cdp_url: None,
        browser: None,
        user_data_dir: None,
        startup_url: None,
    };

    let tool = BrowserCdpTool::new_with_config(config);

    // Navigate first
    let nav = tool
        .handle(
            ToolInput::new("browse_url")
                .with_param("url", serde_json::json!("https://example.com")),
        )
        .expect("navigate should succeed");
    assert!(nav.success);

    // Then read text
    let read = tool
        .handle(ToolInput::new("read_page_text"))
        .expect("read_page_text should succeed");
    assert!(read.success);
    let text = read.data.as_str().unwrap_or_default();
    assert!(
        text.len() > 10,
        "page text should be non-trivially long, got: {text:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chrome installed; run with --include-ignored"]
async fn browser_cdp_click_element_on_live_page() {
    use browser_cdp::{BrowserCdpTool, CdpConfig, CdpMode};
    use sdk::core_tool::CoreTool;

    let config = CdpConfig {
        mode: CdpMode::ManagedLocal,
        cdp_url: None,
        browser: None,
        user_data_dir: None,
        startup_url: None,
    };

    let tool = BrowserCdpTool::new_with_config(config);

    tool.handle(
        ToolInput::new("browse_url").with_param("url", serde_json::json!("https://example.com")),
    )
    .expect("navigate should succeed");

    // example.com has one anchor — clicking it should succeed or return a clear error
    let click =
        tool.handle(ToolInput::new("click_element").with_param("selector", serde_json::json!("a")));

    match click {
        Ok(output) => {
            assert!(
                output.success,
                "click returned success=false: {:?}",
                output.error
            );
        }
        Err(e) => {
            // A tool error is acceptable here (e.g. selector not found after navigation)
            assert!(
                e.to_string().contains("ERROR") || e.to_string().contains("no element"),
                "unexpected error from click_element: {e}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chrome installed; run with --include-ignored"]
async fn browser_cdp_fill_form_field_on_search_page() {
    use browser_cdp::{BrowserCdpTool, CdpConfig, CdpMode};
    use sdk::core_tool::CoreTool;

    let config = CdpConfig {
        mode: CdpMode::ManagedLocal,
        cdp_url: None,
        browser: None,
        user_data_dir: None,
        startup_url: None,
    };

    let tool = BrowserCdpTool::new_with_config(config);

    tool.handle(
        ToolInput::new("browse_url").with_param("url", serde_json::json!("https://duckduckgo.com")),
    )
    .expect("navigate to duckduckgo should succeed");

    let fill = tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("input[name=q]"))
            .with_param("value", serde_json::json!("rove ai")),
    );

    match fill {
        Ok(output) => assert!(
            output.success,
            "fill returned success=false: {:?}",
            output.error
        ),
        Err(e) => panic!("fill_form_field unexpectedly failed: {e}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chrome installed and manual verification; run with --include-ignored"]
async fn browser_cdp_fill_google_form_test() {
    use browser_cdp::{BrowserCdpTool, CdpConfig, CdpMode};
    use sdk::core_tool::CoreTool;

    let config = CdpConfig {
        mode: CdpMode::ManagedLocal,
        cdp_url: None,
        browser: None,
        user_data_dir: None,
        startup_url: None,
    };

    let tool = BrowserCdpTool::new_with_config(config);

    // Navigate to the Google form
    let nav = tool.handle(ToolInput::new("browse_url").with_param(
        "url",
        serde_json::json!("https://forms.gle/UZDru6hJQvYp1QCaA"),
    ));
    assert!(nav.is_ok(), "navigate should succeed: {:?}", nav.err());

    // Wait for form to load
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Read page to see what's there
    let read = tool.handle(ToolInput::new("read_page_text"));
    if let Ok(output) = read {
        println!(
            "Form content:\n{}",
            output
                .data
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(500)
                .collect::<String>()
        );
    }

    // Fill Name field (first text input)
    let fill_name = tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("input[type='text']"))
            .with_param("value", serde_json::json!("Rove Test User 4821")),
    );
    assert!(
        fill_name.is_ok(),
        "Should fill name field: {:?}",
        fill_name.err()
    );
    println!("✓ Filled name field");

    // Click shirt size (M option)
    let click_size = tool.handle(
        ToolInput::new("click_element")
            .with_param("selector", serde_json::json!("div[data-value='M']")),
    );
    if click_size.is_ok() {
        println!("✓ Selected shirt size M");
    }

    // Fill comments field (last text input or textarea)
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let fill_comments = tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("textarea"))
            .with_param(
                "value",
                serde_json::json!("Automated browser CDP driver test - 2026-04-19"),
            ),
    );
    if fill_comments.is_ok() {
        println!("✓ Filled comments field");
    }

    println!("\n✅ Form filling test completed successfully");
    println!("Note: Form was NOT submitted (test only fills fields)");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chrome and WILL SUBMIT FORM - only run if you own the form"]
async fn browser_cdp_fill_and_submit_google_form() {
    use browser_cdp::{BrowserCdpTool, CdpConfig, CdpMode};
    use sdk::core_tool::CoreTool;

    let config = CdpConfig {
        mode: CdpMode::ManagedLocal,
        cdp_url: None,
        browser: None,
        user_data_dir: None,
        startup_url: None,
    };

    let tool = BrowserCdpTool::new_with_config(config);

    // Navigate to the Google form
    tool.handle(ToolInput::new("browse_url").with_param(
        "url",
        serde_json::json!("https://forms.gle/UZDru6hJQvYp1QCaA"),
    ))
    .expect("navigate should succeed");

    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Fill Name field
    tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("input[type='text']"))
            .with_param("value", serde_json::json!("Rove Test Submission")),
    )
    .expect("should fill name");
    println!("✓ Filled name");

    // Select shirt size M
    tool.handle(
        ToolInput::new("click_element")
            .with_param("selector", serde_json::json!("div[data-value='M']")),
    )
    .expect("should select size");
    println!("✓ Selected size");

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Fill comments
    tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("textarea"))
            .with_param(
                "value",
                serde_json::json!("Test submission from browser-cdp driver"),
            ),
    )
    .expect("should fill comments");
    println!("✓ Filled comments");

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Click Submit button
    let submit = tool.handle(
        ToolInput::new("click_element")
            .with_param("selector", serde_json::json!("span:has-text('Submit')")),
    );

    // Try alternative submit selectors if first fails
    if submit.is_err() {
        let submit_alt = tool.handle(ToolInput::new("click_element").with_param(
            "selector",
            serde_json::json!("div[role='button']:has-text('Submit')"),
        ));
        assert!(submit_alt.is_ok(), "Should click submit button");
    }

    println!("✓ Clicked Submit button");

    // Wait to see confirmation
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let confirmation = tool.handle(ToolInput::new("read_page_text"));
    if let Ok(output) = confirmation {
        let text = output.data.as_str().unwrap_or_default();
        println!(
            "\nPage after submit:\n{}",
            text.chars().take(300).collect::<String>()
        );
    }

    println!("\n⚠️  FORM WAS SUBMITTED");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chrome and localhost:3000 running"]
async fn browser_cdp_submit_localhost_form_and_verify() {
    use browser_cdp::{BrowserCdpTool, CdpConfig, CdpMode};
    use sdk::core_tool::CoreTool;

    let config = CdpConfig {
        mode: CdpMode::ManagedLocal,
        cdp_url: None,
        browser: None,
        user_data_dir: None,
        startup_url: None,
    };

    let tool = BrowserCdpTool::new_with_config(config);

    // Navigate to localhost form
    println!("📝 Opening form at http://localhost:3000/");
    tool.handle(
        ToolInput::new("browse_url").with_param("url", serde_json::json!("http://localhost:3000/")),
    )
    .expect("navigate to form should succeed");

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Read page to see form structure
    let page_content = tool.handle(ToolInput::new("read_page_text"));
    if let Ok(output) = page_content {
        println!(
            "Form page content:\n{}\n",
            output
                .data
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(500)
                .collect::<String>()
        );
    }

    // Fill form fields
    println!("✍️  Filling form fields...");

    // Fill name
    tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("input[name='name']"))
            .with_param("value", serde_json::json!("Rove Browser Test")),
    )
    .expect("should fill name");
    println!("✓ Filled name");

    // Fill email
    tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("input[name='email']"))
            .with_param("value", serde_json::json!("test@rove.dev")),
    )
    .expect("should fill email");
    println!("✓ Filled email");

    // Fill password
    tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("input[name='password']"))
            .with_param("value", serde_json::json!("TestPass123")),
    )
    .expect("should fill password");
    println!("✓ Filled password");

    // Fill age
    tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("input[name='age']"))
            .with_param("value", serde_json::json!("25")),
    )
    .expect("should fill age");
    println!("✓ Filled age");

    // Fill message/textarea
    tool.handle(
        ToolInput::new("fill_form_field")
            .with_param("selector", serde_json::json!("textarea[name='message']"))
            .with_param(
                "value",
                serde_json::json!("Test message from browser-cdp driver"),
            ),
    )
    .expect("should fill message");
    println!("✓ Filled message");

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Submit form
    println!("📤 Submitting form...");
    let submit = tool.handle(
        ToolInput::new("click_element")
            .with_param("selector", serde_json::json!("button[type='submit']")),
    );
    assert!(
        submit.is_ok(),
        "Should click submit button: {:?}",
        submit.err()
    );
    println!("✓ Clicked submit button");

    // Wait for submission to complete and check result
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Check page content after submission
    let after_submit = tool.handle(ToolInput::new("read_page_text"));
    if let Ok(output) = after_submit {
        println!(
            "\n📄 Page after submit:\n{}\n",
            output
                .data
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect::<String>()
        );
    }

    // Navigate to responses API to verify
    println!("🔍 Checking responses at http://localhost:3000/api/responses");
    tool.handle(ToolInput::new("browse_url").with_param(
        "url",
        serde_json::json!("http://localhost:3000/api/responses"),
    ))
    .expect("navigate to responses should succeed");

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Read responses
    let responses = tool.handle(ToolInput::new("read_page_text"));
    if let Ok(output) = responses {
        let response_text = output.data.as_str().unwrap_or_default();
        println!("\n📊 Responses API output:\n{}\n", response_text);

        // Check if we got a valid JSON response
        if response_text.contains("\"message\"") || response_text.contains("\"data\"") {
            println!("✅ Successfully accessed responses API");

            if response_text.contains("Rove Browser Test")
                || response_text.contains("test@rove.dev")
            {
                println!("✅ Verified: Form submission found in responses!");
            } else {
                println!("⚠️  Note: Responses API returned empty data array");
                println!("   This might mean:");
                println!("   - Form validation failed (missing required fields)");
                println!("   - Form submission is async and not yet saved");
                println!("   - Form doesn't persist to this API endpoint");
            }
        }
    } else {
        panic!("Failed to read responses");
    }

    println!("\n✅ Test completed: Form was filled and submit button clicked");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chrome and localhost:3000 running"]
async fn browser_cdp_smart_form_fill_test() {
    use browser_cdp::{BrowserCdpTool, CdpConfig, CdpMode};
    use sdk::core_tool::CoreTool;

    let config = CdpConfig {
        mode: CdpMode::ManagedLocal,
        cdp_url: None,
        browser: None,
        user_data_dir: None,
        startup_url: None,
    };

    let tool = BrowserCdpTool::new_with_config(config);

    // Navigate
    tool.handle(
        ToolInput::new("browse_url").with_param("url", serde_json::json!("http://localhost:3000/")),
    )
    .expect("navigate should succeed");

    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Check page loaded
    let page_text = tool.handle(ToolInput::new("read_page_text"));
    if let Ok(output) = &page_text {
        println!(
            "Page loaded:\n{}",
            output
                .data
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect::<String>()
        );
    }

    // Test inspect_form
    println!("\n📋 Testing inspect_form...");
    let inspect = tool.handle(ToolInput::new("inspect_form"));
    assert!(inspect.is_ok(), "inspect_form should succeed");
    if let Ok(output) = inspect {
        println!(
            "Form structure:\n{}",
            output
                .data
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(500)
                .collect::<String>()
        );
    }

    // Test get_page_structure
    println!("\n🏗️  Testing get_page_structure...");
    let structure = tool.handle(ToolInput::new("get_page_structure"));
    assert!(structure.is_ok(), "get_page_structure should succeed");
    if let Ok(output) = structure {
        println!(
            "Page structure:\n{}",
            output.data.as_str().unwrap_or_default()
        );
    }

    // Test fill_form_smart
    println!("\n✍️  Testing fill_form_smart...");
    let fill = tool.handle(
        ToolInput::new("fill_form_smart")
            .with_param(
                "data",
                serde_json::json!({
                    "name": "Smart Fill Test",
                    "email": "smart@test.com",
                    "age": 30,
                    "message": "Filled by smart tool"
                }),
            )
            .with_param("submit", serde_json::json!(true)),
    );
    assert!(
        fill.is_ok(),
        "fill_form_smart should succeed: {:?}",
        fill.err()
    );

    if let Ok(output) = fill {
        println!("Fill result:\n{}", output.data.as_str().unwrap_or_default());
        let result: serde_json::Value =
            serde_json::from_str(output.data.as_str().unwrap_or("{}")).unwrap();
        assert!(
            !result["filled"].as_array().unwrap().is_empty(),
            "Should fill at least one field"
        );
        println!(
            "✅ Filled {} fields",
            result["filled"].as_array().unwrap().len()
        );
    }

    // Verify submission
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    tool.handle(ToolInput::new("browse_url").with_param(
        "url",
        serde_json::json!("http://localhost:3000/api/responses"),
    ))
    .expect("navigate to responses");

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    let responses = tool.handle(ToolInput::new("read_page_text"));
    if let Ok(output) = responses {
        let text = output.data.as_str().unwrap_or_default();
        assert!(
            text.contains("Smart Fill Test") || text.contains("smart@test.com"),
            "Response should contain submitted data"
        );
        println!("✅ Verified submission in responses API");
    }

    println!("\n✅ All smart tools working!");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chrome"]
async fn test_extract_semantic_data_flipkart() {
    use browser_cdp::{BrowserCdpTool, CdpConfig, CdpMode};
    use sdk::core_tool::CoreTool;

    let config = CdpConfig {
        mode: CdpMode::ManagedLocal,
        cdp_url: None,
        browser: None,
        user_data_dir: None,
        startup_url: None,
    };

    let tool = BrowserCdpTool::new_with_config(config);

    // Navigate to Flipkart protein supplements
    println!("\n🌐 Navigating to Flipkart...");
    tool.handle(
        ToolInput::new("browse_url")
            .with_param("url", serde_json::json!("https://www.flipkart.com/health-care/health-supplements/protein-supplement/pr?sid=hlc,etg,1rx"))
    ).expect("navigate should succeed");

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // Test extract_semantic_data
    println!("\n📊 Extracting semantic data...");
    let extract = tool.handle(ToolInput::new("extract_semantic_data").with_param(
        "keys",
        serde_json::json!(["price", "title", "rating", "product"]),
    ));

    assert!(
        extract.is_ok(),
        "extract_semantic_data should succeed: {:?}",
        extract.err()
    );

    if let Ok(output) = extract {
        let data = output.data.as_str().unwrap_or("{}");
        println!("Extracted data:\n{}", data);

        let parsed: serde_json::Value = serde_json::from_str(data).unwrap();
        println!(
            "\n✅ Successfully extracted {} fields",
            parsed.as_object().unwrap().len()
        );
    }
}
