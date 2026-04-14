use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use semver::Version;
use serde::Deserialize;
use tracing::warn;

use crate::config::Config;
use crate::runtime::{Manifest, PluginType, TrustTier};
use crate::security::crypto::CryptoModule;
use crate::storage::{Database, ExtensionCatalogEntry, InstalledPlugin};
use sdk::{
    CatalogExtensionRecord, CatalogVersionRecord, ExtensionProvenance, ExtensionTrustBadge,
    ExtensionUpdateRecord,
};

use super::install::{install_checked, upgrade_checked};
use super::inventory::open_database;
use super::registry::{
    load_registry_catalog, load_registry_plugin_index, read_registry_text, select_registry_version,
    verify_signed_registry_json,
};

const PUBLIC_EXTENSION_CATALOG_URL: &str = "https://registry.roveai.co/extensions";
const CATALOG_REFRESH_INTERVAL_SECS: i64 = 15 * 60;

#[derive(Debug, Deserialize)]
struct RegistryReleaseMetadata {
    permission_review: super::validate::PermissionReview,
}

pub(crate) fn public_catalog_registry() -> String {
    std::env::var("ROVE_PUBLIC_EXTENSION_CATALOG")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| PUBLIC_EXTENSION_CATALOG_URL.to_string())
}

pub(crate) async fn list_catalog(
    config: &Config,
    force_refresh: bool,
) -> Result<Vec<CatalogExtensionRecord>> {
    let database = open_database(config).await?;
    let cache = maybe_refresh_catalog_cache(config, &database, force_refresh).await?;
    let installed = database
        .installed_plugins()
        .list_plugins()
        .await
        .context("Failed to list installed plugins while building catalog view")?;

    Ok(cache
        .into_iter()
        .map(|entry| catalog_record_from_entry(&entry, &installed))
        .collect())
}

pub(crate) async fn get_catalog_entry(
    config: &Config,
    id: &str,
    force_refresh: bool,
) -> Result<CatalogExtensionRecord> {
    let database = open_database(config).await?;
    let cache = maybe_refresh_catalog_cache(config, &database, force_refresh).await?;
    let installed = database
        .installed_plugins()
        .list_plugins()
        .await
        .context("Failed to list installed plugins while building catalog detail")?;
    cache
        .into_iter()
        .find(|entry| entry.id == id)
        .map(|entry| catalog_record_from_entry(&entry, &installed))
        .with_context(|| format!("Extension '{}' was not found in the public catalog", id))
}

pub(crate) async fn list_updates(
    config: &Config,
    force_refresh: bool,
) -> Result<Vec<ExtensionUpdateRecord>> {
    let database = open_database(config).await?;
    let cache = maybe_refresh_catalog_cache(config, &database, force_refresh).await?;
    let installed = database
        .installed_plugins()
        .list_plugins()
        .await
        .context("Failed to list installed plugins while computing updates")?;

    let mut updates = Vec::new();
    for plugin in installed {
        let Some(entry) = cache.iter().find(|entry| entry.id == plugin.id) else {
            continue;
        };
        if !version_is_newer(&plugin.version, &entry.latest_version) {
            continue;
        }

        updates.push(ExtensionUpdateRecord {
            id: entry.id.clone(),
            name: entry.name.clone(),
            kind: entry.kind.clone(),
            installed_version: plugin.version.clone(),
            latest_version: entry.latest_version.clone(),
            trust_badge: trust_badge_from_str(&entry.trust_badge),
            provenance: provenance_for_catalog(&entry.registry_source),
            permission_summary: entry.permission_summary.clone(),
            permission_warnings: entry.permission_warnings.clone(),
            release_summary: entry.release_summary.clone(),
        });
    }

    updates.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(updates)
}

pub(crate) async fn install_with_catalog_defaults(
    config: &Config,
    source: &str,
    registry: Option<&str>,
    version: Option<&str>,
    expected_type: Option<PluginType>,
) -> Result<InstalledPlugin> {
    let database = open_database(config).await?;
    let plan = resolve_install_plan(config, &database, source, registry, version).await?;
    maybe_warn_advanced_install(&plan);

    let mut installed = install_checked(
        config,
        &plan.resolved_source,
        plan.registry.as_deref(),
        version,
        expected_type,
    )
    .await?;

    apply_provenance(&mut installed, &plan);
    database
        .installed_plugins()
        .upsert_plugin(&installed)
        .await
        .context("Failed to persist extension install provenance")?;

    Ok(installed)
}

pub(crate) async fn upgrade_with_catalog_defaults(
    config: &Config,
    source: &str,
    registry: Option<&str>,
    version: Option<&str>,
    expected_type: Option<PluginType>,
) -> Result<InstalledPlugin> {
    let database = open_database(config).await?;
    let plan = resolve_install_plan(config, &database, source, registry, version).await?;
    maybe_warn_advanced_install(&plan);

    let mut installed = upgrade_checked(
        config,
        &plan.resolved_source,
        plan.registry.as_deref(),
        version,
        expected_type,
    )
    .await?;

    apply_provenance(&mut installed, &plan);
    database
        .installed_plugins()
        .upsert_plugin(&installed)
        .await
        .context("Failed to persist extension upgrade provenance")?;

    Ok(installed)
}

async fn maybe_refresh_catalog_cache(
    config: &Config,
    database: &Database,
    force_refresh: bool,
) -> Result<Vec<ExtensionCatalogEntry>> {
    let last_fetched = database.extension_catalog().last_fetched_at().await?;
    let now = unix_now()?;
    let should_refresh = force_refresh
        || last_fetched.is_none()
        || now - last_fetched.unwrap_or_default() >= CATALOG_REFRESH_INTERVAL_SECS;

    if should_refresh {
        match refresh_catalog_cache(config, database).await {
            Ok(entries) => return Ok(entries),
            Err(error) => {
                let cached = database.extension_catalog().list_entries().await?;
                warn!(
                    registry = %public_catalog_registry(),
                    cached_entries = cached.len(),
                    "extension catalog refresh failed, falling back to cached data: {error}"
                );
                return Ok(cached);
            }
        }
    }

    database.extension_catalog().list_entries().await
}

async fn refresh_catalog_cache(
    _config: &Config,
    database: &Database,
) -> Result<Vec<ExtensionCatalogEntry>> {
    let registry = public_catalog_registry();
    let catalog = load_registry_catalog(&registry).await?;
    let crypto = CryptoModule::new().context("Failed to initialize extension catalog verifier")?;
    let fetched_at = unix_now()?;
    let mut entries = Vec::new();

    for plugin in catalog.plugins {
        let trust_badge = trust_badge_from_registry_tier(&plugin.trust_tier);
        if matches!(trust_badge, ExtensionTrustBadge::Unverified) {
            continue;
        }

        let index = load_registry_plugin_index(&registry, &plugin.id).await?;
        let latest = select_registry_version(&index, None)?;
        let manifest_raw = read_registry_text(&registry, &latest.manifest_path).await?;
        crypto
            .verify_manifest_file(manifest_raw.as_bytes())
            .with_context(|| format!("Catalog manifest verification failed for '{}'", plugin.id))?;
        let manifest = Manifest::from_json(&manifest_raw)
            .with_context(|| format!("Invalid catalog manifest for '{}'", plugin.id))?;

        let release_raw = read_registry_text(&registry, &latest.release_path).await?;
        if registry.starts_with("https://") || registry.starts_with("http://") {
            verify_signed_registry_json(
                &release_raw,
                &format!("release metadata for {}", plugin.id),
            )?;
        }
        let release: RegistryReleaseMetadata = serde_json::from_str(&release_raw)
            .with_context(|| format!("Invalid release metadata for '{}'", plugin.id))?;

        let release_summary = if let Some(readme_path) = &latest.readme_path {
            summarize_release_notes(&read_registry_text(&registry, readme_path).await?)
        } else {
            None
        };

        entries.push(ExtensionCatalogEntry {
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            kind: public_kind_from_plugin_type(&plugin.plugin_type).to_string(),
            description: manifest.description.clone(),
            trust_badge: trust_badge.as_str().to_string(),
            latest_version: latest.version.clone(),
            latest_published_at: latest.published_at,
            registry_source: registry.clone(),
            index_path: plugin.index_path.clone(),
            manifest_json: manifest_raw,
            permission_summary: release.permission_review.summary_lines,
            permission_warnings: release.permission_review.warnings,
            release_summary,
            fetched_at,
        });
    }

    database.extension_catalog().replace_all(&entries).await?;
    Ok(entries)
}

struct InstallPlan {
    resolved_source: String,
    registry: Option<String>,
    provenance_source: String,
    provenance_registry: Option<String>,
    trust_badge: Option<String>,
    advanced: bool,
}

async fn resolve_install_plan(
    config: &Config,
    database: &Database,
    source: &str,
    registry: Option<&str>,
    version: Option<&str>,
) -> Result<InstallPlan> {
    let source = source.trim();
    let source_path_exists = Path::new(source).exists();

    if let Some(registry) = registry {
        ensure_developer_mode(config, "installing from an explicit registry")?;
        let trust_badge =
            if normalize_registry(registry) == normalize_registry(&public_catalog_registry()) {
                let entry = get_catalog_entry(config, source, false).await.ok();
                entry.map(|entry| entry.trust_badge.as_str().to_string())
            } else {
                Some(ExtensionTrustBadge::Unverified.as_str().to_string())
            };
        return Ok(InstallPlan {
            resolved_source: source.to_string(),
            registry: Some(registry.to_string()),
            provenance_source: "explicit_registry".to_string(),
            provenance_registry: Some(registry.to_string()),
            trust_badge,
            advanced: true,
        });
    }

    if source_path_exists {
        ensure_developer_mode(config, "installing from a local package directory")?;
        return Ok(InstallPlan {
            resolved_source: source.to_string(),
            registry: None,
            provenance_source: "local_package".to_string(),
            provenance_registry: None,
            trust_badge: Some(ExtensionTrustBadge::Unverified.as_str().to_string()),
            advanced: true,
        });
    }

    let entry = get_catalog_entry(config, source, false)
        .await
        .with_context(|| {
            format!(
                "Extension '{}' is not available in the Rove catalog",
                source
            )
        })?;
    if let Some(requested) = version {
        let _ = requested;
    }
    let installed = database
        .installed_plugins()
        .get_plugin(&entry.id)
        .await
        .context("Failed to inspect installed extension state")?;

    Ok(InstallPlan {
        resolved_source: entry.id.clone(),
        registry: Some(public_catalog_registry()),
        provenance_source: if installed.is_some() {
            "public_catalog_upgrade".to_string()
        } else {
            "public_catalog".to_string()
        },
        provenance_registry: Some(public_catalog_registry()),
        trust_badge: Some(entry.trust_badge.as_str().to_string()),
        advanced: false,
    })
}

fn apply_provenance(installed: &mut InstalledPlugin, plan: &InstallPlan) {
    installed.provenance_source = Some(plan.provenance_source.clone());
    installed.provenance_registry = plan.provenance_registry.clone();
    installed.catalog_trust_badge = plan.trust_badge.clone();
}

fn ensure_developer_mode(config: &Config, action: &str) -> Result<()> {
    if config.daemon.developer_mode {
        return Ok(());
    }
    bail!(
        "Developer mode is required for {}. Enable it with `rove config` or the WebUI settings.",
        action
    )
}

fn maybe_warn_advanced_install(plan: &InstallPlan) {
    if !plan.advanced {
        return;
    }

    eprintln!(
        "warning: installing from '{}' bypasses the default public catalog review path",
        plan.provenance_source
    );
}

fn normalize_registry(registry: &str) -> String {
    registry.trim_end_matches('/').to_string()
}

fn catalog_record_from_entry(
    entry: &ExtensionCatalogEntry,
    installed: &[InstalledPlugin],
) -> CatalogExtensionRecord {
    let installed_plugin = installed.iter().find(|plugin| plugin.id == entry.id);
    CatalogExtensionRecord {
        id: entry.id.clone(),
        name: entry.name.clone(),
        kind: entry.kind.clone(),
        description: entry.description.clone(),
        trust_badge: trust_badge_from_str(&entry.trust_badge),
        provenance: provenance_for_catalog(&entry.registry_source),
        latest: CatalogVersionRecord {
            version: entry.latest_version.clone(),
            published_at: entry.latest_published_at,
            permission_summary: entry.permission_summary.clone(),
            permission_warnings: entry.permission_warnings.clone(),
            release_summary: entry.release_summary.clone(),
        },
        installed: installed_plugin.is_some(),
        installed_version: installed_plugin.map(|plugin| plugin.version.clone()),
        update_available: installed_plugin
            .map(|plugin| version_is_newer(&plugin.version, &entry.latest_version))
            .unwrap_or(false),
    }
}

pub(crate) fn provenance_for_catalog(registry: &str) -> ExtensionProvenance {
    ExtensionProvenance {
        source: "public_catalog".to_string(),
        registry: Some(registry.to_string()),
        catalog_managed: true,
        advanced_source: false,
    }
}

pub(crate) fn trust_badge_from_str(value: &str) -> ExtensionTrustBadge {
    match value.trim().to_ascii_lowercase().as_str() {
        "official" => ExtensionTrustBadge::Official,
        "verified" => ExtensionTrustBadge::Verified,
        _ => ExtensionTrustBadge::Unverified,
    }
}

fn trust_badge_from_registry_tier(value: &str) -> ExtensionTrustBadge {
    match value.trim().to_ascii_lowercase().as_str() {
        "official" => ExtensionTrustBadge::Official,
        "reviewed" => ExtensionTrustBadge::Verified,
        "community" => ExtensionTrustBadge::Unverified,
        _ => ExtensionTrustBadge::Unverified,
    }
}

pub(crate) fn trust_badge_from_manifest_tier(value: TrustTier) -> ExtensionTrustBadge {
    match value {
        TrustTier::Official => ExtensionTrustBadge::Official,
        TrustTier::Reviewed => ExtensionTrustBadge::Verified,
        TrustTier::Community => ExtensionTrustBadge::Unverified,
    }
}

pub(crate) fn public_kind_from_plugin_type(plugin_type: &str) -> &'static str {
    match plugin_type {
        "Skill" => "skill",
        "Workspace" => "system",
        "Channel" => "channel",
        "Mcp" => "connector",
        "Brain" => "brain",
        _ => "extension",
    }
}

fn summarize_release_notes(readme: &str) -> Option<String> {
    let summary = readme
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    if summary.is_empty() {
        None
    } else {
        Some(summary.chars().take(240).collect())
    }
}

fn version_is_newer(current: &str, latest: &str) -> bool {
    match (Version::parse(current), Version::parse(latest)) {
        (Ok(current), Ok(latest)) => latest > current,
        _ => current != latest,
    }
}

fn unix_now() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock before UNIX_EPOCH")?
        .as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::{public_kind_from_plugin_type, summarize_release_notes, version_is_newer};

    #[test]
    fn plugin_type_maps_to_public_kind() {
        assert_eq!(public_kind_from_plugin_type("Skill"), "skill");
        assert_eq!(public_kind_from_plugin_type("Workspace"), "system");
        assert_eq!(public_kind_from_plugin_type("Channel"), "channel");
    }

    #[test]
    fn release_summary_skips_headings() {
        let summary = summarize_release_notes("# Heading\n\nAdds a safer shell.\nImproves docs.");
        assert_eq!(
            summary.as_deref(),
            Some("Adds a safer shell. Improves docs.")
        );
    }

    #[test]
    fn semver_update_detection_prefers_newer_versions() {
        assert!(version_is_newer("0.1.0", "0.2.0"));
        assert!(!version_is_newer("0.2.0", "0.2.0"));
    }
}
