//! Lightweight update-availability probe used by the HTTP API.
//!
//! Fetches the signed channel manifest, verifies it, and reports whether a
//! newer engine version is published. Results are cached for 30 minutes so
//! the WebUI / CLI can poll cheaply.
//!
//! **Read-only**: never self-replaces the binary. Stable-channel users must
//! trigger the actual update with `rove update`.

use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::cli::update::{channel_manifest_signature_url, channel_manifest_url, verify_manifest_signature};
use crate::config::channel::Channel;
use crate::config::metadata::{user_agent, VERSION};

const CACHE_TTL_SECS: u64 = 30 * 60;

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub channel: String,
    pub current: String,
    pub latest: String,
    pub update_available: bool,
    pub manifest_url: String,
    pub checked_at: i64,
}

#[derive(Debug, Deserialize)]
struct RegistryManifest {
    #[serde(default)]
    channel: Option<String>,
    engines: std::collections::HashMap<String, EngineRelease>,
}

#[derive(Debug, Deserialize)]
struct EngineRelease {
    version: String,
}

struct CacheEntry {
    status: UpdateStatus,
    cached_at: SystemTime,
}

fn cache() -> &'static Mutex<Option<CacheEntry>> {
    static CACHE: OnceLock<Arc<Mutex<Option<CacheEntry>>>> = OnceLock::new();
    CACHE
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .as_ref()
}

pub async fn check_update_available() -> Result<UpdateStatus> {
    let cache_lock = cache();
    {
        let guard = cache_lock.lock().await;
        if let Some(entry) = guard.as_ref() {
            if entry
                .cached_at
                .elapsed()
                .map(|elapsed| elapsed.as_secs() < CACHE_TTL_SECS)
                .unwrap_or(false)
            {
                return Ok(entry.status.clone());
            }
        }
    }

    let status = fetch_and_verify().await?;

    let mut guard = cache_lock.lock().await;
    *guard = Some(CacheEntry {
        status: status.clone(),
        cached_at: SystemTime::now(),
    });

    Ok(status)
}

async fn fetch_and_verify() -> Result<UpdateStatus> {
    let channel = Channel::current();
    let manifest_url = channel_manifest_url(channel);
    let signature_url = channel_manifest_signature_url(channel);

    let client = reqwest::Client::builder().user_agent(user_agent()).build()?;

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

    let latest_version = manifest
        .engines
        .get("latest")
        .map(|release| release.version.as_str())
        .ok_or_else(|| anyhow::anyhow!("Manifest missing 'latest' engine entry"))?;

    let current = semver::Version::parse(VERSION).context("invalid engine VERSION constant")?;
    let latest = semver::Version::parse(latest_version)
        .context("invalid 'latest' version in manifest")?;

    let checked_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    Ok(UpdateStatus {
        channel: channel.as_str().to_string(),
        current: current.to_string(),
        latest: latest.to_string(),
        update_available: latest > current,
        manifest_url,
        checked_at,
    })
}

pub async fn invalidate_cache() {
    let mut guard = cache().lock().await;
    *guard = None;
}

#[cfg(test)]
mod tests {
    use super::UpdateStatus;
    use serde_json;

    #[test]
    fn update_status_serializes_with_snake_case_fields() {
        let status = UpdateStatus {
            channel: "dev".to_string(),
            current: "0.0.3".to_string(),
            latest: "0.0.4".to_string(),
            update_available: true,
            manifest_url: "https://example".to_string(),
            checked_at: 1_700_000_000,
        };
        let v = serde_json::to_value(&status).unwrap();
        assert_eq!(v["channel"], "dev");
        assert_eq!(v["update_available"], true);
        assert_eq!(v["current"], "0.0.3");
    }
}
