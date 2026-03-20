use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::config::Config;
use crate::runtime::{McpServerConfig, SandboxProfile, SDK_VERSION};

use super::templates::load_templates;
use super::ScaffoldRequest;

pub(super) fn generate_package(config: &Config, request: ScaffoldRequest) -> Result<()> {
    let templates = load_templates(config)?;
    let template = templates
        .get(&request.template)
        .with_context(|| format!("unknown MCP template '{}'", request.template))?;

    ensure_scaffold_directory(&request.dir)?;

    let server_name = request
        .server_name
        .clone()
        .unwrap_or_else(|| default_id(&request.name));
    let package_id = default_id(&request.name);

    let command = request
        .command
        .clone()
        .or_else(|| template.command.clone())
        .unwrap_or_else(|| "REPLACE_WITH_MCP_COMMAND".to_string());
    let args = if request.args.is_empty() {
        template.args.clone()
    } else {
        request.args.clone()
    };

    let mut profile = template.profile.clone();
    if request.allow_network {
        profile.allow_network = true;
    }
    if request.allow_tmp {
        profile.allow_tmp = true;
    }
    profile.read_paths.extend(request.read_paths.clone());
    profile.write_paths.extend(request.write_paths.clone());
    normalize_profile(&mut profile);

    let description = request
        .description
        .clone()
        .unwrap_or_else(|| template.description.clone());

    let runtime = McpServerConfig {
        name: server_name.clone(),
        template: Some(template.key.clone()),
        description: Some(description.clone()),
        command,
        args,
        profile: profile.clone(),
        cached_tools: Vec::new(),
        enabled: true,
    };

    let manifest = json!({
        "name": request.name,
        "version": "0.1.0",
        "sdk_version": SDK_VERSION,
        "plugin_type": "Mcp",
        "permissions": {
            "filesystem": profile_filesystem_permissions(&profile),
            "network": profile_network_permissions(&profile),
            "memory_read": false,
            "memory_write": false,
            "tools": []
        },
        "trust_tier": "Community",
        "min_model": null,
        "description": description,
        "signature": "REPLACE_WITH_MANIFEST_SIGNATURE"
    });

    let package = json!({
        "id": package_id,
        "runtime_config": "runtime.json",
        "payload_hash": "REPLACE_WITH_SHA256_OF_RUNTIME_JSON",
        "payload_signature": "REPLACE_WITH_SIGNATURE_OF_RUNTIME_JSON",
        "enabled": true
    });

    fs::write(
        request.dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )
    .with_context(|| {
        format!(
            "Failed to write '{}'",
            request.dir.join("manifest.json").display()
        )
    })?;
    fs::write(
        request.dir.join("plugin-package.json"),
        serde_json::to_string_pretty(&package)?,
    )
    .with_context(|| {
        format!(
            "Failed to write '{}'",
            request.dir.join("plugin-package.json").display()
        )
    })?;
    fs::write(
        request.dir.join("runtime.json"),
        serde_json::to_string_pretty(&runtime)?,
    )
    .with_context(|| {
        format!(
            "Failed to write '{}'",
            request.dir.join("runtime.json").display()
        )
    })?;
    fs::write(
        request.dir.join("README.md"),
        scaffold_readme(
            &request.dir,
            template.key.as_str(),
            &server_name,
            &description,
            &profile,
        ),
    )
    .with_context(|| {
        format!(
            "Failed to write '{}'",
            request.dir.join("README.md").display()
        )
    })?;

    println!("Created MCP package scaffold in {}", request.dir.display());
    println!("Generated files:");
    println!("- {}", request.dir.join("manifest.json").display());
    println!("- {}", request.dir.join("plugin-package.json").display());
    println!("- {}", request.dir.join("runtime.json").display());
    println!("- {}", request.dir.join("README.md").display());
    println!("Next: edit runtime.json, compute hash/signature values, sign manifest.json, then run `rove mcp install {}`.", request.dir.display());

    Ok(())
}

fn ensure_scaffold_directory(dir: &Path) -> Result<()> {
    if dir.exists() {
        if !dir.is_dir() {
            bail!("Scaffold target '{}' is not a directory", dir.display());
        }
        if dir.read_dir()?.next().is_some() {
            bail!(
                "Scaffold target '{}' is not empty; use an empty directory",
                dir.display()
            );
        }
        return Ok(());
    }

    fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create scaffold directory '{}'", dir.display()))
}

fn default_id(name: &str) -> String {
    let mut id = String::new();
    let mut last_dash = false;

    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            id.push(lower);
            last_dash = false;
        } else if !last_dash {
            id.push('-');
            last_dash = true;
        }
    }

    let normalized = id.trim_matches('-').to_string();
    if normalized.is_empty() {
        "mcp-package".to_string()
    } else {
        normalized
    }
}

fn normalize_profile(profile: &mut SandboxProfile) {
    profile.read_paths = dedup_paths(&profile.read_paths);
    profile.write_paths = dedup_paths(&profile.write_paths);
}

fn dedup_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().to_string();
        if seen.insert(key) {
            unique.push(path.clone());
        }
    }
    unique
}

fn profile_filesystem_permissions(profile: &SandboxProfile) -> Vec<String> {
    profile
        .read_paths
        .iter()
        .chain(profile.write_paths.iter())
        .map(|path| path.display().to_string())
        .collect()
}

fn profile_network_permissions(profile: &SandboxProfile) -> Vec<String> {
    if profile.allow_network {
        vec!["REPLACE_WITH_ALLOWED_DOMAIN".to_string()]
    } else {
        Vec::new()
    }
}

fn scaffold_readme(
    dir: &Path,
    template: &str,
    server_name: &str,
    description: &str,
    profile: &SandboxProfile,
) -> String {
    let mut lines = vec![
        "# MCP Package Scaffold".to_string(),
        String::new(),
        format!("Directory: `{}`", dir.display()),
        format!("Template: `{}`", template),
        format!("Server name: `{}`", server_name),
        format!("Description: {}", description),
        String::new(),
        "Files:".to_string(),
        "- `manifest.json`: plugin manifest with placeholder signature.".to_string(),
        "- `plugin-package.json`: install metadata with placeholder payload hash/signature.".to_string(),
        "- `runtime.json`: MCP runtime configuration consumed by Rove.".to_string(),
        String::new(),
        "Before install:".to_string(),
        "- Edit `runtime.json` so the command, args, sandbox profile, and description are correct.".to_string(),
        "- Replace placeholder filesystem/network permissions in `manifest.json` with the explicit paths and domains your package needs.".to_string(),
        "- Compute the SHA256 of `runtime.json` and place it in `plugin-package.json` as `payload_hash`.".to_string(),
        "- Sign `runtime.json` and place the signature in `plugin-package.json` as `payload_signature`.".to_string(),
        "- Sign `manifest.json` and replace `signature` with the real manifest signature.".to_string(),
        String::new(),
        "Install locally:".to_string(),
        format!("- `rove mcp install {}`", dir.display()),
        String::new(),
        "Current scaffold sandbox:".to_string(),
        format!("- network: {}", profile.allow_network),
        format!("- tmp: {}", profile.allow_tmp),
    ];

    if profile.read_paths.is_empty() {
        lines.push("- read_paths: (none)".to_string());
    } else {
        lines.push("- read_paths:".to_string());
        for path in &profile.read_paths {
            lines.push(format!("  - {}", path.display()));
        }
    }

    if profile.write_paths.is_empty() {
        lines.push("- write_paths: (none)".to_string());
    } else {
        lines.push("- write_paths:".to_string());
        for path in &profile.write_paths {
            lines.push(format!("  - {}", path.display()));
        }
    }

    lines.push(String::new());
    lines.push("This scaffold is meant for authoring. It will not install until the placeholder hash and signatures are replaced with real values.".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::config::Config;

    use super::{default_id, generate_package};
    use crate::cli::mcp::ScaffoldRequest;

    #[test]
    fn default_id_normalizes_names() {
        assert_eq!(default_id("GitHub Connector"), "github-connector");
        assert_eq!(default_id("!!!"), "mcp-package");
    }

    #[test]
    fn generate_package_writes_scaffold_files() {
        let temp_dir = TempDir::new().expect("temp dir");
        let output_dir = temp_dir.path().join("github-mcp");

        let config = Config::default();
        generate_package(
            &config,
            ScaffoldRequest {
                dir: output_dir.clone(),
                name: "GitHub MCP".to_string(),
                template: "github".to_string(),
                server_name: Some("github".to_string()),
                command: Some("github-mcp".to_string()),
                args: vec!["stdio".to_string()],
                description: Some("GitHub package".to_string()),
                allow_network: true,
                allow_tmp: true,
                read_paths: Vec::new(),
                write_paths: Vec::new(),
            },
        )
        .expect("generate scaffold");

        assert!(output_dir.join("manifest.json").exists());
        assert!(output_dir.join("plugin-package.json").exists());
        assert!(output_dir.join("runtime.json").exists());
        assert!(output_dir.join("README.md").exists());

        let runtime = fs::read_to_string(output_dir.join("runtime.json")).expect("runtime file");
        assert!(runtime.contains("\"name\": \"github\""));
        assert!(runtime.contains("\"command\": \"github-mcp\""));
    }
}
