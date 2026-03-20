use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::cli::commands::PluginScaffoldType;
use crate::runtime::SDK_VERSION;

use super::package::{default_plugin_id, MANIFEST_FILE, PACKAGE_FILE, RUNTIME_FILE};

pub async fn handle_new(name: &str, plugin_type: PluginScaffoldType) -> Result<()> {
    let dir = PathBuf::from(name);
    let package_name = display_name_for_path(&dir);
    generate_plugin_scaffold(&dir, &package_name, plugin_type)?;

    println!("Created plugin scaffold in {}", dir.display());
    println!("Generated files:");
    println!("- {}", dir.join("Cargo.toml").display());
    println!("- {}", dir.join(MANIFEST_FILE).display());
    println!("- {}", dir.join(PACKAGE_FILE).display());
    println!("- {}", dir.join(RUNTIME_FILE).display());
    println!("- {}", dir.join("src/lib.rs").display());
    println!("- {}", dir.join("tests/integration.rs").display());
    println!("- {}", dir.join("README.md").display());
    println!(
        "Next: cargo test, cargo build --target wasm32-wasip1 --release, rove plugin test {} --input \"hello\", then replace placeholder hash/signatures before install.",
        dir.display()
    );

    Ok(())
}

pub fn generate_plugin_scaffold(
    dir: &Path,
    package_name: &str,
    plugin_type: PluginScaffoldType,
) -> Result<()> {
    ensure_scaffold_directory(dir)?;

    let crate_slug = default_plugin_id(package_name).replace('-', "_");
    let install_id = default_plugin_id(package_name);
    let plugin_type_name = plugin_type_name(plugin_type);
    let artifact = format!("target/wasm32-wasip1/release/{}.wasm", crate_slug);
    let tool_name = "run";

    fs::create_dir_all(dir.join("src"))
        .with_context(|| format!("Failed to create '{}'", dir.join("src").display()))?;
    fs::create_dir_all(dir.join("tests"))
        .with_context(|| format!("Failed to create '{}'", dir.join("tests").display()))?;

    fs::write(
        dir.join("Cargo.toml"),
        cargo_toml(&crate_slug, package_name, plugin_type_name),
    )
    .with_context(|| format!("Failed to write '{}'", dir.join("Cargo.toml").display()))?;
    fs::write(dir.join(".gitignore"), "target/\n*.wasm\n.DS_Store\n")
        .with_context(|| format!("Failed to write '{}'", dir.join(".gitignore").display()))?;
    fs::write(
        dir.join(MANIFEST_FILE),
        serde_json::to_string_pretty(&manifest_json(package_name, plugin_type_name))?,
    )
    .with_context(|| format!("Failed to write '{}'", dir.join(MANIFEST_FILE).display()))?;
    fs::write(
        dir.join(PACKAGE_FILE),
        serde_json::to_string_pretty(&package_json(&install_id, &artifact))?,
    )
    .with_context(|| format!("Failed to write '{}'", dir.join(PACKAGE_FILE).display()))?;
    fs::write(
        dir.join(RUNTIME_FILE),
        serde_json::to_string_pretty(&runtime_json(package_name, tool_name))?,
    )
    .with_context(|| format!("Failed to write '{}'", dir.join(RUNTIME_FILE).display()))?;
    fs::write(dir.join("src/lib.rs"), lib_rs(tool_name, plugin_type_name))
        .with_context(|| format!("Failed to write '{}'", dir.join("src/lib.rs").display()))?;
    fs::write(
        dir.join("tests/integration.rs"),
        integration_test_rs(&crate_slug),
    )
    .with_context(|| {
        format!(
            "Failed to write '{}'",
            dir.join("tests/integration.rs").display()
        )
    })?;
    fs::write(
        dir.join("README.md"),
        readme(dir, package_name, plugin_type_name, &artifact),
    )
    .with_context(|| format!("Failed to write '{}'", dir.join("README.md").display()))?;

    Ok(())
}

fn ensure_scaffold_directory(dir: &Path) -> Result<()> {
    if dir.exists() {
        if !dir.is_dir() {
            bail!(
                "Plugin scaffold target '{}' is not a directory",
                dir.display()
            );
        }
        if dir.read_dir()?.next().is_some() {
            bail!(
                "Plugin scaffold target '{}' is not empty; use an empty directory",
                dir.display()
            );
        }
        return Ok(());
    }

    fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create scaffold directory '{}'", dir.display()))
}

fn display_name_for_path(path: &Path) -> String {
    let raw = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("plugin");

    raw.split(['-', '_', ' '])
        .filter(|part| !part.is_empty())
        .map(title_case)
        .collect::<Vec<_>>()
        .join(" ")
}

fn title_case(part: &str) -> String {
    let mut chars = part.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn plugin_type_name(plugin_type: PluginScaffoldType) -> &'static str {
    match plugin_type {
        PluginScaffoldType::Skill => "Skill",
        PluginScaffoldType::Channel => "Channel",
    }
}

fn cargo_toml(crate_slug: &str, package_name: &str, plugin_type: &str) -> String {
    format!(
        r#"[package]
name = "{crate_slug}"
version = "0.1.0"
edition = "2021"
description = "{package_name} {plugin_type} plugin for Rove"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
extism-pdk = "1"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#
    )
}

fn manifest_json(package_name: &str, plugin_type: &str) -> serde_json::Value {
    json!({
        "name": package_name,
        "version": "0.1.0",
        "sdk_version": SDK_VERSION,
        "plugin_type": plugin_type,
        "permissions": {
            "filesystem": [],
            "network": [],
            "memory_read": false,
            "memory_write": false,
            "tools": []
        },
        "trust_tier": "Community",
        "min_model": null,
        "description": format!("{package_name} plugin for Rove"),
        "signature": "LOCAL_DEV_MANIFEST_SIGNATURE"
    })
}

fn package_json(install_id: &str, artifact: &str) -> serde_json::Value {
    json!({
        "id": install_id,
        "artifact": artifact,
        "runtime_config": "runtime.json",
        "payload_hash": "LOCAL_DEV_PAYLOAD_HASH",
        "payload_signature": "LOCAL_DEV_PAYLOAD_SIGNATURE",
        "enabled": true
    })
}

fn runtime_json(package_name: &str, tool_name: &str) -> serde_json::Value {
    json!({
        "tools": [
            {
                "name": tool_name,
                "description": format!("Execute the main entry point for {}", package_name),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "input": { "type": "string", "description": "Primary task input" },
                        "file": { "type": "string", "description": "Single file path" },
                        "files": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Additional file paths"
                        }
                    }
                },
                "domains": ["all"]
            }
        ]
    })
}

fn lib_rs(tool_name: &str, plugin_type: &str) -> String {
    format!(
        r#"use serde::{{Deserialize, Serialize}};
use serde_json::Value;

#[cfg(target_arch = "wasm32")]
use extism_pdk::*;

#[derive(Debug, Deserialize, Default)]
pub struct RunInput {{
    #[serde(default)]
    pub input: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub context: Value,
}}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RunOutput {{
    pub summary: String,
    pub files_seen: Vec<String>,
    pub plugin_type: String,
}}

pub fn run_impl(input: RunInput) -> RunOutput {{
    let mut files = input.files;
    if let Some(file) = input.file {{
        if !files.iter().any(|existing| existing == &file) {{
            files.push(file);
        }}
    }}

    let action = if input.input.trim().is_empty() {{
        "Replace this scaffold with your real plugin logic.".to_string()
    }} else {{
        format!("Received task: {{}}", input.input.trim())
    }};

    RunOutput {{
        summary: action,
        files_seen: files,
        plugin_type: "{plugin_type}".to_string(),
    }}
}}

#[cfg(target_arch = "wasm32")]
#[plugin_fn]
pub fn {tool_name}(Json(input): Json<RunInput>) -> FnResult<Json<RunOutput>> {{
    Ok(Json(run_impl(input)))
}}
"#
    )
    .replace("{plugin_type}", plugin_type)
}

fn integration_test_rs(crate_slug: &str) -> String {
    format!(
        r#"use {crate_slug}::{{run_impl, RunInput}};

#[test]
fn run_impl_reports_input_and_files() {{
    let output = run_impl(RunInput {{
        input: "summarise this".to_string(),
        file: Some("sample.pdf".to_string()),
        files: vec!["notes.txt".to_string()],
        context: serde_json::Value::Null,
    }});

    assert!(output.summary.contains("summarise this"));
    assert_eq!(output.files_seen.len(), 2);
    assert!(output.files_seen.contains(&"sample.pdf".to_string()));
}}
"#
    )
}

fn readme(dir: &Path, package_name: &str, plugin_type: &str, artifact: &str) -> String {
    let dir_display = dir.display();
    format!(
        "# {package_name}\n\n\
This is a generated {plugin_type} plugin scaffold for Rove.\n\n\
## Files\n\n\
- `Cargo.toml` - Rust crate configured for `wasm32-wasip1`\n\
- `manifest.json` - plugin manifest with placeholder signature\n\
- `plugin-package.json` - install metadata with placeholder hash/signature\n\
- `runtime.json` - tool catalog consumed by Rove\n\
- `src/lib.rs` - plugin entry point exporting `run`\n\
- `tests/integration.rs` - local unit test for the scaffold logic\n\n\
## Authoring loop\n\n\
1. `rustup target add wasm32-wasip1`\n\
2. `cargo test`\n\
3. `cargo build --target wasm32-wasip1 --release`\n\
4. `rove plugin test {dir_display} --input \"hello\"`\n\n\
## Before install\n\n\
1. Build the wasm artifact at `{artifact}`\n\
2. Replace the placeholder permissions in `manifest.json`\n\
3. Compute the SHA256 of the built artifact and place it in `plugin-package.json`\n\
4. Sign the built artifact and place the signature in `plugin-package.json`\n\
5. Sign `manifest.json` and replace `signature`\n\
6. Install with `rove plugin install {dir_display}`\n"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::generate_plugin_scaffold;
    use crate::cli::commands::PluginScaffoldType;

    #[test]
    fn generate_scaffold_writes_expected_files() {
        let temp_dir = TempDir::new().expect("temp dir");
        let output_dir = temp_dir.path().join("my-pdf-reader");

        generate_plugin_scaffold(&output_dir, "My Pdf Reader", PluginScaffoldType::Skill)
            .expect("generate scaffold");

        assert!(output_dir.join("Cargo.toml").exists());
        assert!(output_dir.join(".gitignore").exists());
        assert!(output_dir.join("manifest.json").exists());
        assert!(output_dir.join("plugin-package.json").exists());
        assert!(output_dir.join("runtime.json").exists());
        assert!(output_dir.join("src/lib.rs").exists());
        assert!(output_dir.join("tests/integration.rs").exists());
        assert!(output_dir.join("README.md").exists());

        let runtime = fs::read_to_string(output_dir.join("runtime.json")).expect("runtime");
        assert!(runtime.contains("\"name\": \"run\""));
    }
}
