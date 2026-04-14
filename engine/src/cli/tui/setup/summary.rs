use super::result::SetupResult;
use super::{BOLD, CYAN, DIM, GREEN, RESET, YELLOW};

pub fn print_summary(result: &SetupResult, config_path: &str, db_ready: bool) {
    println!();
    println!("  {CYAN}{BOLD}┌─ Setup Complete ─────────────────────────────────┐{RESET}");
    println!("  {CYAN}│{RESET}                                                  {CYAN}│{RESET}");
    println!(
        "  {CYAN}│{RESET}  {DIM}Workspace{RESET}    {BOLD}{:<35}{RESET} {CYAN}│{RESET}",
        result.workspace
    );

    if result.skipped_model {
        println!(
            "  {CYAN}│{RESET}  {DIM}Provider{RESET}     {YELLOW}{:<35}{RESET} {CYAN}│{RESET}",
            "skipped (configure later)"
        );
    } else {
        println!(
            "  {CYAN}│{RESET}  {DIM}Provider{RESET}     {BOLD}{:<35}{RESET} {CYAN}│{RESET}",
            result.provider_name
        );
        println!(
            "  {CYAN}│{RESET}  {DIM}Model{RESET}        {:<35} {CYAN}│{RESET}",
            result.model
        );
        if !result.api_key.is_empty() {
            println!(
                "  {CYAN}│{RESET}  {DIM}API Key{RESET}      {GREEN}{:<35}{RESET} {CYAN}│{RESET}",
                format!("{} captured", mask_key(&result.api_key))
            );
        }
    }

    println!(
        "  {CYAN}│{RESET}  {DIM}Risk Tier{RESET}    {:<35} {CYAN}│{RESET}",
        result.max_risk_tier
    );
    if let Some(auth_protection) = &result.auth_protection {
        println!(
            "  {CYAN}│{RESET}  {DIM}Auth{RESET}         {:<35} {CYAN}│{RESET}",
            auth_protection
        );
    }
    println!(
        "  {CYAN}│{RESET}  {DIM}Config{RESET}       {:<35} {CYAN}│{RESET}",
        config_path
    );
    if db_ready {
        println!(
            "  {CYAN}│{RESET}  {DIM}Database{RESET}     {GREEN}{:<35}{RESET} {CYAN}│{RESET}",
            "ready"
        );
    }
    println!("  {CYAN}│{RESET}                                                  {CYAN}│{RESET}");
    println!("  {CYAN}│{RESET}  Run {BOLD}`rove start`{RESET} to begin.                      {CYAN}│{RESET}");
    println!("  {CYAN}{BOLD}└──────────────────────────────────────────────────┘{RESET}");
    println!();
    if let Some(recovery_code) = &result.recovery_code {
        println!("  {BOLD}Recovery code{RESET}: {GREEN}{recovery_code}{RESET}");
        println!(
            "  {DIM}Store this once-only code somewhere safe. It can reset the local daemon password if the device seal is unavailable.{RESET}"
        );
        println!();
    }
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }

    let prefix = &key[..4];
    let suffix = &key[key.len() - 3..];
    format!("{}...{}", prefix, suffix)
}
