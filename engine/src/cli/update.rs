use anyhow::{Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;

use crate::config::channel::Channel;
use crate::config::metadata::{user_agent, VERSION};
use crate::security::crypto::CryptoModule;

use super::output::OutputFormat;

const REGISTRY_BASE: &str = "https://registry.roveai.co";
const REGISTRY_BASE_ENV: &str = "ROVE_REGISTRY_BASE";

// Schema v2 — produced by the update-registry CI job.
// Layout: entries.latest.version + entries.latest.platforms[target]
#[derive(Debug, Deserialize)]
struct RegistryManifest {
    #[serde(default)]
    channel: Option<String>,
    entries: EntriesV2,
}

#[derive(Debug, Deserialize)]
struct EntriesV2 {
    latest: LatestEntry,
}

#[derive(Debug, Deserialize)]
struct LatestEntry {
    version: String,
    #[serde(default)]
    platforms: std::collections::HashMap<String, PlatformRelease>,
}

#[derive(Debug, Deserialize)]
struct PlatformRelease {
    url: String,
    fallback_url: Option<String>,
    blake3: String,
    size_bytes: Option<u64>,
}

pub async fn handle_update(check_only: bool, format: OutputFormat) -> Result<()> {
    let channel = Channel::current();
    let current = semver::Version::parse(VERSION).context("Failed to parse current version")?;

    let client = reqwest::Client::builder()
        .user_agent(user_agent())
        .build()?;
    let manifest_url = channel_manifest_url(channel);
    let signature_url = channel_manifest_signature_url(channel);

    let manifest_text = client
        .get(&manifest_url)
        .send()
        .await?
        .error_for_status()
        .context("Failed to fetch registry manifest")?
        .text()
        .await?;

    let signature_hex = client
        .get(&signature_url)
        .send()
        .await?
        .error_for_status()
        .context("Failed to fetch registry manifest signature")?
        .text()
        .await?;

    verify_manifest_signature(manifest_text.as_bytes(), signature_hex.trim())
        .context("Registry manifest signature verification failed")?;

    let manifest: RegistryManifest =
        serde_json::from_str(&manifest_text).context("Failed to parse registry manifest JSON")?;

    if let Some(claimed) = manifest.channel.as_deref() {
        if !claimed.eq_ignore_ascii_case(channel.as_str()) {
            anyhow::bail!(
                "Manifest channel mismatch: expected '{}', got '{}'",
                channel.as_str(),
                claimed
            );
        }
    }

    let latest_version = manifest.entries.latest.version.as_str();
    let latest =
        semver::Version::parse(latest_version).context("Failed to parse latest version")?;

    if latest <= current {
        return print_up_to_date(&current, &latest, format);
    }

    print_available(&current, &latest, &manifest_url, format)?;
    if check_only {
        return Ok(());
    }

    let target = current_manifest_target();
    let platform_release = manifest
        .entries
        .latest
        .platforms
        .get(target)
        .ok_or_else(|| anyhow::anyhow!("No release found for target '{}'", target))?;

    let download_url = platform_release
        .fallback_url
        .as_deref()
        .unwrap_or(platform_release.url.as_str());
    let payload = download_payload(
        &client,
        download_url,
        platform_release.size_bytes.unwrap_or(0),
    )
    .await?;

    verify_payload(platform_release, &payload)?;
    replace_binary(target, &payload)?;

    match format {
        OutputFormat::Text => println!(
            "Successfully updated Rove ({} channel): v{} -> v{}",
            channel.as_str(),
            current,
            latest
        ),
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "updated",
                "channel": channel.as_str(),
                "previous_version": current.to_string(),
                "new_version": latest.to_string(),
            }))?
        ),
    }

    Ok(())
}

pub fn channel_manifest_url(channel: Channel) -> String {
    format!(
        "{}/{}/engine/manifest.json",
        registry_base(),
        channel.as_str()
    )
}

pub fn channel_manifest_signature_url(channel: Channel) -> String {
    format!(
        "{}/{}/engine/manifest.sig",
        registry_base(),
        channel.as_str()
    )
}

fn registry_base() -> String {
    std::env::var(REGISTRY_BASE_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .unwrap_or_else(|| REGISTRY_BASE.to_string())
}

pub fn verify_manifest_signature(manifest_bytes: &[u8], signature_hex: &str) -> Result<()> {
    let crypto = CryptoModule::new().context("Failed to load embedded team public key")?;
    let canonical = CryptoModule::canonicalize_manifest(manifest_bytes)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    crypto
        .verify_manifest(&canonical, signature_hex)
        .map_err(|error| anyhow::anyhow!("{}", error))
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
    println!(
        "Downloading payload ({:.1} MB)...",
        size_bytes as f64 / 1_048_576.0
    );

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

fn verify_payload(release: &PlatformRelease, payload: &[u8]) -> Result<()> {
    eprintln!("Verifying payload integrity...");

    let computed = CryptoModule::compute_blake3(payload);
    if computed != release.blake3 {
        anyhow::bail!(
            "Hash mismatch.\nExpected: {}\nComputed: {}",
            release.blake3,
            computed
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

#[cfg(test)]
mod tests {
    use super::{channel_manifest_signature_url, channel_manifest_url, verify_manifest_signature};
    use crate::config::channel::Channel;

    #[test]
    fn manifest_url_uses_channel_segment() {
        assert!(channel_manifest_url(Channel::Stable).contains("/stable/"));
        assert!(channel_manifest_url(Channel::Dev).contains("/dev/"));
        assert!(channel_manifest_signature_url(Channel::Dev).ends_with("manifest.sig"));
    }

    #[test]
    fn manifest_url_honors_registry_base_override() {
        let previous = std::env::var_os("ROVE_REGISTRY_BASE");
        std::env::set_var("ROVE_REGISTRY_BASE", "https://mirror.example.com");
        let url = channel_manifest_url(Channel::Stable);
        assert!(url.starts_with("https://mirror.example.com/stable/engine/manifest.json"));
        match previous {
            Some(value) => std::env::set_var("ROVE_REGISTRY_BASE", value),
            None => std::env::remove_var("ROVE_REGISTRY_BASE"),
        }
    }

    #[test]
    fn rejects_obviously_bad_signature() {
        let err = verify_manifest_signature(b"{}", "ed25519:00").expect_err("short sig must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("Signature") || msg.contains("signature"),
            "unexpected error: {}",
            msg
        );
    }
}
