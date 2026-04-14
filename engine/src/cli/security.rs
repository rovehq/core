use anyhow::Result;

use crate::config::Config;
use crate::security::{password_protection_state, PasswordProtectionState};

pub async fn show_security() -> Result<()> {
    let config = Config::load_or_create()?;

    // ── Auth / Password ──────────────────────────────────────────
    let password_state =
        password_protection_state(&config).unwrap_or(PasswordProtectionState::Uninitialized);

    println!();
    println!("  Rove Security Posture");
    println!("  =====================");
    println!();

    println!("  Auth:");
    println!("    Password seal:  {}", state_label(password_state));

    // ── Approvals ────────────────────────────────────────────────
    let approvals = &config.approvals;
    println!();
    println!("  Approvals:");
    println!("    Mode:             {}", approvals.mode.as_str());
    println!(
        "    Remote admin:     {}",
        bool_label(approvals.allow_remote_admin_approvals)
    );
    if let Some(ref rules_path) = approvals.rules_path {
        println!("    Rules file:     {}", rules_path.display());
    } else {
        println!("    Rules file:     (default — internal rules)");
    }

    // ── Risk Tiers ───────────────────────────────────────────────
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

    // ── Secret backend ───────────────────────────────────────────
    println!();
    println!("  Secrets:");
    println!("    Backend:          {}", config.secrets.backend.as_str());

    // ── Sandbox ──────────────────────────────────────────────────
    println!();
    println!("  Sandboxing:");
    println!("    MCP:              {}", mcp_sandbox_label());
    println!("    WASM imports:     allowlisted");

    // ── Trust model ──────────────────────────────────────────────
    println!();
    println!("  Trust model:");
    println!("    Extensions:       signed catalog with trust badges");
    println!("    Remote nodes:     Ed25519 pairing + nonce replay protection");
    println!("    Imports:          provenance-tracked, disabled by default");

    println!();
    Ok(())
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
