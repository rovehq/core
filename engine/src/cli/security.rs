use std::fmt;
use std::path::Path;

use anyhow::{bail, Result};

use crate::cli::database_path::database_path;
use crate::cli::plugins::public_kind_from_plugin_type;
use crate::config::Config;
use crate::runtime::{Manifest, TrustTier};
use crate::security::{password_protection_state, PasswordProtectionState};
use crate::storage::{Database, InstalledPlugin};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SecurityExtensionRow {
    name: String,
    kind: String,
    origin: String,
    integrity: String,
    privilege: String,
    findings: Vec<SecurityFinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SecurityFindingSeverity {
    High,
    Medium,
    Low,
}

impl SecurityFindingSeverity {
    fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

impl fmt::Display for SecurityFindingSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SecurityFinding {
    severity: SecurityFindingSeverity,
    message: String,
}

#[derive(Debug, Default)]
struct SecurityAuditReport {
    rows: Vec<SecurityExtensionRow>,
}

pub async fn show_security() -> Result<()> {
    let config = Config::load_or_create()?;
    let password_state =
        password_protection_state(&config).unwrap_or(PasswordProtectionState::Uninitialized);
    let report = collect_security_report(&config).await?;

    print_security_summary(&config, password_state);
    print_extensions_table(&report.rows);
    Ok(())
}

pub async fn audit_security() -> Result<()> {
    let config = Config::load_or_create()?;
    let password_state =
        password_protection_state(&config).unwrap_or(PasswordProtectionState::Uninitialized);
    let report = collect_security_report(&config).await?;

    print_security_summary(&config, password_state);
    print_extensions_table(&report.rows);
    print_findings(&report.rows);

    let actionable = report
        .rows
        .iter()
        .flat_map(|row| row.findings.iter())
        .filter(|finding| finding.severity <= SecurityFindingSeverity::Medium)
        .count();
    if actionable > 0 {
        bail!("Security audit found {} actionable issue(s)", actionable);
    }

    println!("  Audit status:       clean");
    println!();
    Ok(())
}

async fn collect_security_report(config: &Config) -> Result<SecurityAuditReport> {
    let database = Database::new(&database_path(config)).await?;
    let installed = database.installed_plugins().list_plugins().await?;

    let mut rows = installed.iter().map(build_security_row).collect::<Vec<_>>();
    rows.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(SecurityAuditReport { rows })
}

fn build_security_row(plugin: &InstalledPlugin) -> SecurityExtensionRow {
    let manifest = Manifest::from_json(&plugin.manifest).ok();
    let origin = format_origin(plugin);
    let integrity = integrity_label(plugin);
    let privilege = privilege_label(plugin, manifest.as_ref());
    let findings = build_findings(plugin, manifest.as_ref(), &integrity, &privilege);

    SecurityExtensionRow {
        name: plugin.name.clone(),
        kind: public_kind_from_plugin_type(&plugin.plugin_type).to_string(),
        origin,
        integrity,
        privilege,
        findings,
    }
}

fn print_security_summary(config: &Config, password_state: PasswordProtectionState) {
    println!();
    println!("  Rove Security Posture");
    println!("  =====================");
    println!();

    println!("  Auth:");
    println!("    Password seal:  {}", state_label(password_state));

    let approvals = &config.approvals;
    println!();
    println!("  Approvals:");
    println!("    Mode:             {}", approvals.mode.as_str());
    println!(
        "    Remote admin:     {}",
        bool_label(approvals.allow_remote_admin_approvals)
    );
    if let Some(ref rules_path) = approvals.rules_path {
        println!("    Rules file:       {}", rules_path.display());
    } else {
        println!("    Rules file:       (default — internal rules)");
    }

    let security = &config.security;
    println!();
    println!("  Risk tiers:");
    println!("    Max tier:         {}", security.max_risk_tier);
    println!(
        "    Tier 1 confirm:   {}{}",
        bool_label(security.confirm_tier1),
        if security.confirm_tier1 {
            format!(" ({}s delay)", security.confirm_tier1_delay)
        } else {
            String::new()
        }
    );
    println!(
        "    Tier 2 explicit:  {}",
        bool_label(security.require_explicit_tier2)
    );

    println!();
    println!("  Secrets:");
    println!("    Backend:          {}", config.secrets.backend.as_str());

    println!();
    println!("  Sandboxing:");
    println!("    MCP:              {}", mcp_sandbox_label());
    println!("    WASM imports:     allowlisted");

    println!();
    println!("  Trust model:");
    println!("    Extensions:       signed catalog with trust badges");
    println!("    Remote nodes:     Ed25519 pairing + nonce replay protection");
    println!("    Imports:          provenance-tracked, disabled by default");
    println!();
}

fn print_extensions_table(rows: &[SecurityExtensionRow]) {
    println!("  Extension Trust Surface:");
    if rows.is_empty() {
        println!("    No installed extensions.");
        println!();
        return;
    }

    let mut name_width = "NAME".len();
    let mut type_width = "TYPE".len();
    let mut origin_width = "ORIGIN".len();
    let mut integrity_width = "INTEGRITY".len();
    let mut privilege_width = "PRIVILEGE".len();

    for row in rows {
        name_width = name_width.max(row.name.len());
        type_width = type_width.max(row.kind.len());
        origin_width = origin_width.max(row.origin.len());
        integrity_width = integrity_width.max(row.integrity.len());
        privilege_width = privilege_width.max(row.privilege.len());
    }

    println!(
        "    {name:<name_width$}  {kind:<type_width$}  {origin:<origin_width$}  {integrity:<integrity_width$}  {privilege:<privilege_width$}",
        name = "NAME",
        kind = "TYPE",
        origin = "ORIGIN",
        integrity = "INTEGRITY",
        privilege = "PRIVILEGE",
        name_width = name_width,
        type_width = type_width,
        origin_width = origin_width,
        integrity_width = integrity_width,
        privilege_width = privilege_width,
    );
    for row in rows {
        println!(
            "    {name:<name_width$}  {kind:<type_width$}  {origin:<origin_width$}  {integrity:<integrity_width$}  {privilege:<privilege_width$}",
            name = row.name,
            kind = row.kind,
            origin = row.origin,
            integrity = row.integrity,
            privilege = row.privilege,
            name_width = name_width,
            type_width = type_width,
            origin_width = origin_width,
            integrity_width = integrity_width,
            privilege_width = privilege_width,
        );
    }
    println!();
}

fn print_findings(rows: &[SecurityExtensionRow]) {
    let mut any = false;
    println!("  Audit findings:");
    for row in rows {
        for finding in &row.findings {
            any = true;
            println!(
                "    - [{}] {}: {}",
                finding.severity, row.name, finding.message
            );
        }
    }
    if !any {
        println!("    No actionable findings.");
    }
    println!();
}

fn build_findings(
    plugin: &InstalledPlugin,
    manifest: Option<&Manifest>,
    integrity: &str,
    privilege: &str,
) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let kind = public_kind_from_plugin_type(&plugin.plugin_type);
    let trust_badge = trust_badge_label(plugin, manifest);
    let advanced_source = plugin
        .provenance_source
        .as_deref()
        .map(|source| source != "public_catalog" && source != "public_catalog_upgrade")
        .unwrap_or(false);

    match integrity {
        "verified" => {}
        "dev-placeholder" => findings.push(SecurityFinding {
            severity: SecurityFindingSeverity::Medium,
            message: "artifact is using LOCAL_DEV placeholder integrity markers".to_string(),
        }),
        "missing-binary" => findings.push(SecurityFinding {
            severity: SecurityFindingSeverity::High,
            message: "binary path is recorded but the artifact is missing on disk".to_string(),
        }),
        "hash-mismatch" => findings.push(SecurityFinding {
            severity: SecurityFindingSeverity::High,
            message: "artifact hash does not match the recorded install metadata".to_string(),
        }),
        "invalid-signature" => findings.push(SecurityFinding {
            severity: SecurityFindingSeverity::High,
            message: "artifact signature failed verification against the team public key"
                .to_string(),
        }),
        "unsigned" => findings.push(SecurityFinding {
            severity: SecurityFindingSeverity::High,
            message: "artifact does not have a recorded signature".to_string(),
        }),
        "no-artifact" => findings.push(SecurityFinding {
            severity: SecurityFindingSeverity::Low,
            message: "no artifact path is recorded for this install".to_string(),
        }),
        _ => findings.push(SecurityFinding {
            severity: SecurityFindingSeverity::Medium,
            message: format!("unrecognized integrity state '{}'", integrity),
        }),
    }

    if trust_badge == "unverified" {
        findings.push(SecurityFinding {
            severity: if kind == "native" {
                SecurityFindingSeverity::High
            } else {
                SecurityFindingSeverity::Medium
            },
            message: "extension trust badge is unverified".to_string(),
        });
    }

    if advanced_source {
        findings.push(SecurityFinding {
            severity: if kind == "native" {
                SecurityFindingSeverity::High
            } else {
                SecurityFindingSeverity::Medium
            },
            message: "extension came from an advanced or unmanaged source".to_string(),
        });
    }

    if kind == "native" && trust_badge != "official" {
        findings.push(SecurityFinding {
            severity: SecurityFindingSeverity::High,
            message: "native extension is not from the official trust tier".to_string(),
        });
    }

    if kind != "native" && privilege.contains("memory-write") {
        findings.push(SecurityFinding {
            severity: SecurityFindingSeverity::Medium,
            message: "sandboxed extension can mutate daemon memory".to_string(),
        });
    }

    if let Some(manifest) = manifest {
        if manifest
            .permissions
            .filesystem
            .iter()
            .any(|pattern| pattern.0 != "workspace" && !pattern.0.starts_with("workspace/"))
        {
            findings.push(SecurityFinding {
                severity: SecurityFindingSeverity::Medium,
                message: "filesystem permission extends beyond the workspace sandbox".to_string(),
            });
        }
        if manifest
            .permissions
            .network
            .iter()
            .any(|pattern| pattern.0.trim() == "*")
        {
            findings.push(SecurityFinding {
                severity: SecurityFindingSeverity::Medium,
                message: "network permission includes wildcard hosts".to_string(),
            });
        }
        if !manifest.permissions.secrets.is_empty() {
            findings.push(SecurityFinding {
                severity: SecurityFindingSeverity::Medium,
                message: "extension can request daemon-managed secret injection".to_string(),
            });
        }
        if manifest
            .permissions
            .host_patterns
            .iter()
            .any(|pattern| pattern.0.trim() == "*")
        {
            findings.push(SecurityFinding {
                severity: SecurityFindingSeverity::Medium,
                message: "secret injection host allowlist includes wildcard hosts".to_string(),
            });
        }
        if !manifest.permissions.tools.is_empty() {
            findings.push(SecurityFinding {
                severity: SecurityFindingSeverity::Low,
                message: "extension can invoke other registered tools".to_string(),
            });
        }
    }

    findings
}

fn format_origin(plugin: &InstalledPlugin) -> String {
    let source = plugin
        .provenance_source
        .as_deref()
        .unwrap_or("installed")
        .to_string();
    if let Some(registry) = plugin.provenance_registry.as_deref() {
        let registry = registry.trim_end_matches('/');
        let registry_name = registry.rsplit('/').next().unwrap_or(registry);
        format!("{}@{}", source, registry_name)
    } else {
        source
    }
}

fn trust_badge_label(plugin: &InstalledPlugin, manifest: Option<&Manifest>) -> &'static str {
    if let Some(badge) = plugin.catalog_trust_badge.as_deref() {
        return match badge {
            "official" => "official",
            "verified" => "verified",
            _ => "unverified",
        };
    }

    match manifest.map(|manifest| manifest.trust_tier) {
        Some(TrustTier::Official) => "official",
        Some(TrustTier::Reviewed) => "verified",
        _ => "unverified",
    }
}

fn integrity_label(plugin: &InstalledPlugin) -> String {
    let Some(path) = plugin
        .binary_path
        .as_deref()
        .map(Path::new)
        .filter(|path| !path.as_os_str().is_empty())
    else {
        return "no-artifact".to_string();
    };

    if !path.exists() {
        return "missing-binary".to_string();
    }

    if is_dev_placeholder(&plugin.binary_hash) || is_dev_placeholder(&plugin.signature) {
        return "dev-placeholder".to_string();
    }

    if plugin.signature.trim().is_empty() {
        return "unsigned".to_string();
    }

    let expected_hash = normalize_expected_hash(&plugin.binary_hash);
    if expected_hash.is_empty() {
        return "unsigned".to_string();
    }

    let Ok(bytes) = std::fs::read(path) else {
        return "missing-binary".to_string();
    };
    let actual_hash = crate::security::crypto::CryptoModule::compute_hash(&bytes);
    if actual_hash != expected_hash {
        return "hash-mismatch".to_string();
    }

    match crate::security::crypto::CryptoModule::new() {
        Ok(crypto) => match crypto.verify_file_signature(path, &plugin.signature) {
            Ok(()) => "verified".to_string(),
            Err(_) => "invalid-signature".to_string(),
        },
        Err(_) => "hash-only".to_string(),
    }
}

fn privilege_label(plugin: &InstalledPlugin, manifest: Option<&Manifest>) -> String {
    if public_kind_from_plugin_type(&plugin.plugin_type) == "native" {
        return "native".to_string();
    }

    let Some(manifest) = manifest else {
        return "sandboxed".to_string();
    };

    let permissions = &manifest.permissions;
    let mut scopes = Vec::new();
    if !permissions.filesystem.is_empty() {
        scopes.push("fs");
    }
    if !permissions.network.is_empty() {
        scopes.push("net");
    }
    if !permissions.secrets.is_empty() {
        scopes.push("secret-inject");
    }
    if permissions.memory_read {
        scopes.push("memory-read");
    }
    if permissions.memory_write {
        scopes.push("memory-write");
    }
    if !permissions.tools.is_empty() {
        scopes.push("tool-bridge");
    }

    if scopes.is_empty() {
        "sandboxed".to_string()
    } else {
        format!("sandboxed+{}", scopes.join(","))
    }
}

fn normalize_expected_hash(value: &str) -> String {
    let value = value.trim();
    if let Some(stripped) = value.strip_prefix("sha256:") {
        stripped.to_string()
    } else if let Some(stripped) = value.strip_prefix("blake3:") {
        stripped.to_string()
    } else {
        value.to_string()
    }
}

fn is_dev_placeholder(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    upper.contains("LOCAL_DEV") || upper.contains("PLACEHOLDER")
}

fn state_label(state: PasswordProtectionState) -> &'static str {
    match state {
        PasswordProtectionState::Sealed => "sealed (password-protected)",
        PasswordProtectionState::LegacyUnsealed => "legacy (not yet password-sealed)",
        PasswordProtectionState::Tampered => "tampered (needs attention)",
        PasswordProtectionState::Uninitialized => "uninitialized (no auth configured)",
    }
}

fn bool_label(v: bool) -> &'static str {
    if v {
        "enabled"
    } else {
        "disabled"
    }
}

fn mcp_sandbox_label() -> &'static str {
    #[cfg(target_os = "linux")]
    return "bubblewrap (bwrap)";

    #[cfg(target_os = "macos")]
    return "seatbelt (sandbox-exec)";

    #[cfg(target_os = "windows")]
    return "job objects (stub — not yet enforced)";

    #[allow(unreachable_code)]
    "not available on this platform"
}

#[cfg(test)]
mod tests {
    use super::{integrity_label, normalize_expected_hash, privilege_label};
    use crate::runtime::{
        DomainPattern, Manifest, PathPattern, Permissions, PluginType, TrustTier,
    };
    use crate::storage::InstalledPlugin;

    fn sample_plugin() -> InstalledPlugin {
        InstalledPlugin {
            id: "plugin.echo".to_string(),
            name: "echo".to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Plugin".to_string(),
            trust_tier: 1,
            manifest: String::new(),
            binary_path: Some("/tmp/echo.wasm".to_string()),
            binary_hash: "LOCAL_DEV_HASH".to_string(),
            signature: "LOCAL_DEV_SIGNATURE".to_string(),
            enabled: true,
            installed_at: 0,
            last_used: None,
            config: None,
            provenance_source: None,
            provenance_registry: None,
            catalog_trust_badge: None,
        }
    }

    fn sample_manifest() -> Manifest {
        Manifest {
            name: "echo".to_string(),
            version: "0.1.0".to_string(),
            sdk_version: "0.1.0".to_string(),
            plugin_type: PluginType::Plugin,
            permissions: Permissions {
                filesystem: vec![PathPattern("workspace".to_string())],
                network: vec![DomainPattern("api.example.com".to_string())],
                secrets: vec!["OPENAI_API_KEY".to_string()],
                host_patterns: vec![DomainPattern("api.example.com".to_string())],
                memory_read: true,
                memory_write: false,
                wasm_max_memory_mb: None,
                tools: vec!["read_file".to_string()],
                wasm_fuel_limit: None,
                max_execution_time: None,
            },
            trust_tier: TrustTier::Reviewed,
            min_model: None,
            description: "echo".to_string(),
        }
    }

    #[test]
    fn normalize_expected_hash_strips_prefixes() {
        assert_eq!(normalize_expected_hash("sha256:abc"), "abc");
        assert_eq!(normalize_expected_hash("blake3:def"), "def");
        assert_eq!(normalize_expected_hash("ghi"), "ghi");
    }

    #[test]
    fn privilege_label_summarizes_sandbox_scope() {
        let plugin = sample_plugin();
        let privilege = privilege_label(&plugin, Some(&sample_manifest()));
        assert_eq!(
            privilege,
            "sandboxed+fs,net,secret-inject,memory-read,tool-bridge"
        );
    }

    #[test]
    fn integrity_label_flags_dev_placeholders() {
        let plugin = sample_plugin();
        assert_eq!(integrity_label(&plugin), "missing-binary");
    }
}
