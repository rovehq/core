use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use extism::{Manifest as ExtismManifest, Plugin, Wasm};
use serde_json::{Map, Value};

use crate::runtime::{DeclaredTool, PluginType, ToolCatalog};

use super::package::{
    default_runtime_file, load_package, load_runtime_config, manifest_from_signed_json,
    read_required_file, resolve_package_root, MANIFEST_FILE,
};
use super::validate::{print_permission_review, resolve_payload_source, validate_plugin_shape};

pub async fn handle_test(
    source: Option<&str>,
    tool: Option<&str>,
    input: Option<&str>,
    files: &[PathBuf],
    args: &[String],
    no_build: bool,
) -> Result<()> {
    let source_path = match source {
        Some(source) => PathBuf::from(source),
        None => std::env::current_dir().context("Failed to resolve current directory")?,
    };
    let package_root = resolve_package_root(&source_path)?;
    let manifest_raw = read_required_file(&package_root.join(MANIFEST_FILE))?;
    let manifest = manifest_from_signed_json(&manifest_raw)?;
    let package = load_package(&package_root)?;
    let runtime_rel = package
        .runtime_config
        .clone()
        .or_else(|| default_runtime_file(&package_root));
    let runtime_raw = load_runtime_config(&package_root, runtime_rel.as_deref())?;
    validate_plugin_shape(&manifest, runtime_raw.as_deref())?;

    if !matches!(
        manifest.plugin_type,
        PluginType::Skill | PluginType::Channel
    ) {
        bail!(
            "rove plugin test currently supports Skill and Channel packages. '{}' is {}.",
            manifest.name,
            manifest.plugin_type.as_str()
        );
    }

    if !no_build {
        run_cargo(&package_root, &["test"])?;
        ensure_wasm_target_installed()?;
        run_cargo(
            &package_root,
            &["build", "--target", "wasm32-wasip1", "--release"],
        )?;
    }

    let artifact =
        resolve_payload_source(&package_root, &manifest, &package, runtime_rel.as_deref())?
            .context("Plugin test requires a WASM artifact")?;
    let catalog = ToolCatalog::from_json(runtime_raw.as_deref())?;
    let selected_tool = select_tool(&catalog, tool)?;
    let payload = build_input_payload(&package_root, input, files, args)?;
    let output = call_wasm_tool(&artifact, &selected_tool.name, &payload)?;

    println!("Plugin test");
    println!("package: {}", package_root.display());
    println!("plugin: {}", manifest.name);
    println!("type: {}", manifest.plugin_type.as_str());
    println!("tool: {}", selected_tool.name);
    print_permission_review(&manifest);
    println!("input:");
    println!("{}", serde_json::to_string_pretty(&payload)?);
    println!("output:");
    match serde_json::from_slice::<Value>(&output) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value)?),
        Err(_) => println!("{}", String::from_utf8_lossy(&output)),
    }

    Ok(())
}

pub(crate) fn run_cargo(package_root: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(package_root)
        .output()
        .with_context(|| {
            format!(
                "Failed to run 'cargo {}' in '{}'",
                args.join(" "),
                package_root.display()
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    bail!(
        "cargo {} failed in '{}'\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        package_root.display(),
        stdout.trim(),
        stderr.trim()
    )
}

pub(crate) fn ensure_wasm_target_installed() -> Result<()> {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();

    let Ok(output) = output else {
        return Ok(());
    };

    if !output.status.success() {
        return Ok(());
    }

    let installed = String::from_utf8_lossy(&output.stdout);
    if installed.lines().any(|line| line.trim() == "wasm32-wasip1") {
        return Ok(());
    }

    bail!(
        "The wasm32-wasip1 target is not installed. Run `rustup target add wasm32-wasip1` before `rove plugin test`."
    );
}

fn select_tool<'a>(catalog: &'a ToolCatalog, tool: Option<&str>) -> Result<&'a DeclaredTool> {
    if let Some(tool_name) = tool {
        return catalog
            .tools
            .iter()
            .find(|entry| entry.name == tool_name)
            .with_context(|| format!("Tool '{}' is not declared in runtime.json", tool_name));
    }

    match catalog.tools.as_slice() {
        [single] => Ok(single),
        _ => catalog
            .tools
            .iter()
            .find(|entry| entry.name == "run")
            .with_context(|| {
                "runtime.json declares multiple tools; pass --tool to choose one".to_string()
            }),
    }
}

fn build_input_payload(
    package_root: &Path,
    input: Option<&str>,
    files: &[PathBuf],
    args: &[String],
) -> Result<Value> {
    let mut payload = Map::new();

    if let Some(input) = input {
        payload.insert("input".to_string(), Value::String(input.to_string()));
    }

    if !files.is_empty() {
        let resolved = files
            .iter()
            .map(|path| resolve_input_file(package_root, path))
            .collect::<Result<Vec<_>>>()?;
        if let Some(first) = resolved.first() {
            payload.insert("file".to_string(), Value::String(first.clone()));
        }
        payload.insert(
            "files".to_string(),
            Value::Array(resolved.into_iter().map(Value::String).collect()),
        );
    }

    for arg in args {
        let (key, value) = arg.split_once('=').with_context(|| {
            format!(
                "Invalid --arg '{}'. Use key=value so it can be added to the plugin input.",
                arg
            )
        })?;
        payload.insert(key.to_string(), parse_value(value));
    }

    if payload.is_empty() {
        payload.insert(
            "input".to_string(),
            Value::String("plugin test".to_string()),
        );
    }

    Ok(Value::Object(payload))
}

fn resolve_input_file(package_root: &Path, path: &Path) -> Result<String> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        package_root.join(path)
    };

    Ok(resolved.display().to_string())
}

fn parse_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

fn call_wasm_tool(artifact: &Path, tool_name: &str, payload: &Value) -> Result<Vec<u8>> {
    let wasm_bytes = fs::read(artifact)
        .with_context(|| format!("Failed to read plugin artifact '{}'", artifact.display()))?;
    let mut plugin = Plugin::new(ExtismManifest::new([Wasm::data(wasm_bytes)]), [], true)
        .map_err(|error| anyhow::anyhow!("Failed to instantiate plugin: {}", error))?;
    let input =
        serde_json::to_vec(payload).context("Failed to serialize plugin test input payload")?;

    plugin
        .call::<&[u8], Vec<u8>>(tool_name, input.as_slice())
        .map_err(|error| anyhow::anyhow!("Plugin tool '{}' failed: {}", tool_name, error))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;
    use tempfile::TempDir;

    use crate::runtime::ToolCatalog;

    use super::{build_input_payload, parse_value, select_tool};

    #[test]
    fn parse_value_keeps_json_when_possible() {
        assert_eq!(parse_value("true"), json!(true));
        assert_eq!(parse_value("{\"a\":1}"), json!({"a": 1}));
        assert_eq!(parse_value("plain"), json!("plain"));
    }

    #[test]
    fn build_input_payload_merges_input_files_and_args() {
        let temp_dir = TempDir::new().expect("temp dir");
        let payload = build_input_payload(
            temp_dir.path(),
            Some("summarise"),
            &[PathBuf::from("sample.pdf")],
            &["max_pages=5".to_string(), "strict=true".to_string()],
        )
        .expect("payload");

        assert_eq!(payload["input"], json!("summarise"));
        assert_eq!(
            payload["file"],
            json!(temp_dir.path().join("sample.pdf").display().to_string())
        );
        assert_eq!(payload["max_pages"], json!(5));
        assert_eq!(payload["strict"], json!(true));
    }

    #[test]
    fn select_tool_prefers_run_when_multiple_exist() {
        let catalog = ToolCatalog::from_json(Some(
            r#"{"tools":[
                {"name":"analyse","description":"Analyse","parameters":{},"domains":["all"]},
                {"name":"run","description":"Run","parameters":{},"domains":["all"]}
            ]}"#,
        ))
        .expect("catalog");

        let tool = select_tool(&catalog, None).expect("run tool");
        assert_eq!(tool.name, "run");
    }
}
