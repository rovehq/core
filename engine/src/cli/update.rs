use anyhow::{Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;

use crate::config::metadata::{user_agent, VERSION};
use crate::security::crypto::CryptoModule;

use super::output::OutputFormat;

#[derive(Debug, Deserialize)]
struct RegistryManifest {
    version: String,
    engines: std::collections::HashMap<String, EngineRelease>,
}

#[derive(Debug, Deserialize)]
struct EngineRelease {
    version: String,
    url: String,
    fallback_url: Option<String>,
    sha256: String,
    size_bytes: Option<u64>,
}

pub async fn handle_update(check_only: bool, format: OutputFormat) -> Result<()> {
    let current = semver::Version::parse(VERSION).context("Failed to parse current version")?;

    let client = reqwest::Client::builder().user_agent(user_agent()).build()?;
    let manifest_url = "https://raw.githubusercontent.com/orvislab/rove-registry/main/manifest.json";

    let manifest_text = client
        .get(manifest_url)
        .send()
        .await?
        .error_for_status()
        .context("Failed to fetch registry manifest")?
        .text()
        .await?;

    let manifest: RegistryManifest =
        serde_json::from_str(&manifest_text).context("Failed to parse registry manifest JSON")?;

    let latest_version = manifest
        .engines
        .get("latest")
        .map(|release| release.version.as_str())
        .unwrap_or(manifest.version.as_str());
    let latest = semver::Version::parse(latest_version).context("Failed to parse latest version")?;

    if latest <= current {
        return print_up_to_date(&current, &latest, format);
    }

    print_available(&current, &latest, manifest_url, format)?;
    if check_only {
        return Ok(());
    }

    let target = current_manifest_target();
    let engine_release = manifest
        .engines
        .get(target)
        .ok_or_else(|| anyhow::anyhow!("No release found for target '{}'", target))?;

    let download_url = engine_release
        .fallback_url
        .as_deref()
        .unwrap_or(engine_release.url.as_str());
    let payload = download_payload(&client, download_url, engine_release.size_bytes.unwrap_or(0)).await?;

    verify_payload(engine_release, &payload)?;
    replace_binary(target, &payload)?;

    match format {
        OutputFormat::Text => println!("Successfully updated Rove: v{} -> v{}", current, latest),
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "updated",
                "previous_version": current.to_string(),
                "new_version": latest.to_string(),
            }))?
        ),
    }

    Ok(())
}

fn print_up_to_date(
    current: &semver::Version,
    latest: &semver::Version,
    format: OutputFormat,
) -> Result<()> {
    match format {
        OutputFormat::Text => println!("Rove is already up to date (v{}).", current),
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "up_to_date",
                "current_version": current.to_string(),
                "latest_version": latest.to_string(),
            }))?
        ),
    }

    Ok(())
}

fn print_available(
    current: &semver::Version,
    latest: &semver::Version,
    manifest_url: &str,
    format: OutputFormat,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("Update available: v{} -> v{}", current, latest);
            println!("Registry: {}", manifest_url);
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "update_available",
                    "current_version": current.to_string(),
                    "latest_version": latest.to_string(),
                    "release_url": manifest_url,
                }))?
            );
        }
    }

    Ok(())
}

async fn download_payload(client: &reqwest::Client, url: &str, size_bytes: u64) -> Result<Vec<u8>> {
    println!("Downloading payload ({:.1} MB)...", size_bytes as f64 / 1_048_576.0);

    let response = client
        .get(url)
        .send()
        .await?
        .error_for_status()
        .context("Failed to download release payload")?;

    let mut stream = response.bytes_stream();
    let mut payload = Vec::with_capacity(size_bytes as usize);
    let mut downloaded = 0_u64;
    let mut reported = 0_u32;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Error reading download stream")?;
        downloaded += chunk.len() as u64;
        payload.extend_from_slice(&chunk);

        if size_bytes > 0 {
            let pct = (downloaded as f64 / size_bytes as f64 * 100.0) as u32;
            if pct / 10 > reported / 10 {
                eprint!("\r  Progress: {}%", pct);
                reported = pct;
            }
        }
    }

    eprintln!("\r  Progress: 100%");
    Ok(payload)
}

fn verify_payload(release: &EngineRelease, payload: &[u8]) -> Result<()> {
    eprintln!("Verifying payload integrity...");

    let computed_hash = hex::encode(CryptoModule::compute_hash(payload));
    if computed_hash != release.sha256 {
        anyhow::bail!(
            "Hash mismatch.\nExpected: {}\nComputed: {}",
            release.sha256,
            computed_hash
        );
    }

    eprintln!("  Payload BLAKE3 hash: verified");
    Ok(())
}

fn replace_binary(target: &str, payload: &[u8]) -> Result<()> {
    let file_name = format!("rove-{}", target);
    let temp_path = std::env::temp_dir().join(file_name);
    std::fs::write(&temp_path, payload).context("Failed to write temporary update file")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    if let Err(error) = self_replace::self_replace(&temp_path) {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            anyhow::bail!(
                "Permission denied. Run `sudo rove update` or elevate the installation path."
            );
        }
        return Err(error).context("Failed to replace the current binary");
    }

    let _ = std::fs::remove_file(&temp_path);
    Ok(())
}

fn current_manifest_target() -> &'static str {
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        return "linux-x86_64";
    }

    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        return "linux-aarch64";
    }

    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    {
        return "darwin-x86_64";
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        return "darwin-aarch64";
    }

    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    {
        return "windows-x86_64";
    }

    #[cfg(all(target_arch = "aarch64", target_os = "windows"))]
    {
        return "windows-aarch64";
    }

    #[allow(unreachable_code)]
    "unsupported"
}
