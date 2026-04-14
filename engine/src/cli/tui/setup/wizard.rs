use anyhow::Result;
use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};
use std::io::{self, Write};

use super::menu::{select_menu, select_menu_default};
use super::preset::{presets, ModelPreset};
use super::prompt::{print_line, prompt_secret, prompt_text};
use super::result::SetupResult;
use super::{BOLD, CYAN, DIM, RESET};

pub fn run_setup_wizard() -> Result<SetupResult> {
    terminal::enable_raw_mode()?;
    let result = run_wizard_inner();
    terminal::disable_raw_mode()?;
    print!("\r\n");
    io::stdout().flush()?;
    result
}

fn run_wizard_inner() -> Result<SetupResult> {
    let mut stdout = io::stdout();

    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    print_header(&mut stdout)?;

    let workspace = prompt_text(&mut stdout, "Workspace directory", "~/projects")?;
    print_line(&mut stdout, "")?;

    let preset = select_preset(&mut stdout)?;
    let skipped = preset.provider_name.is_empty();

    let (provider_name, protocol, base_url, model, secret_key) = if preset.provider_name == "custom"
    {
        prompt_custom_provider(&mut stdout)?
    } else {
        (
            preset.provider_name.clone(),
            preset.protocol.clone(),
            preset.base_url.clone(),
            preset.model.clone(),
            preset.secret_key.clone(),
        )
    };

    let api_key = if !skipped && preset.needs_api_key {
        let key = prompt_text(&mut stdout, "API key", "")?;
        if !key.is_empty() {
            print_line(&mut stdout, "  Key captured")?;
        }
        print_line(&mut stdout, "")?;
        key
    } else {
        String::new()
    };

    let risk_labels = vec![
        format!("Tier 0  {DIM}Read-only operations only{RESET}"),
        format!("Tier 1  {DIM}Allow local modifications with confirmation{RESET}"),
        format!("Tier 2  {DIM}Allow all operations with confirmation{RESET}"),
    ];
    print_line(&mut stdout, &format!("  {BOLD}Maximum risk tier{RESET}"))?;
    print_line(&mut stdout, "")?;
    let risk_idx = select_menu_default(&mut stdout, &risk_labels, 2)?;
    print_line(&mut stdout, "")?;

    let daemon_password = loop {
        let password = prompt_secret(&mut stdout, "Daemon password")?;
        if password.trim().len() < 8 {
            print_line(
                &mut stdout,
                "  Password must be at least 8 characters. Try again.",
            )?;
            continue;
        }

        let confirm = prompt_secret(&mut stdout, "Confirm daemon password")?;
        if password != confirm {
            print_line(&mut stdout, "  Passwords did not match. Try again.")?;
            continue;
        }
        print_line(&mut stdout, "")?;
        break password;
    };

    Ok(SetupResult {
        workspace,
        provider_name,
        protocol,
        base_url,
        model,
        secret_key,
        api_key,
        max_risk_tier: risk_idx as u8,
        skipped_model: skipped,
        daemon_password,
        recovery_code: None,
        auth_protection: None,
    })
}

fn print_header(stdout: &mut io::Stdout) -> Result<()> {
    print_line(stdout, "")?;
    print_line(
        stdout,
        &format!("  {CYAN}{BOLD}╔══════════════════════════════════╗{RESET}"),
    )?;
    print_line(
        stdout,
        &format!("  {CYAN}{BOLD}║       Rove Setup Wizard          ║{RESET}"),
    )?;
    print_line(
        stdout,
        &format!("  {CYAN}{BOLD}╚══════════════════════════════════╝{RESET}"),
    )?;
    print_line(stdout, "")?;
    Ok(())
}

fn select_preset(stdout: &mut io::Stdout) -> Result<ModelPreset> {
    let all_presets = presets();
    let labels = all_presets
        .iter()
        .map(|preset| format!("{}  {DIM}{}{RESET}", preset.label, preset.description))
        .collect::<Vec<_>>();

    print_line(stdout, &format!("  {BOLD}Quick Model Setup{RESET}"))?;
    print_line(stdout, "")?;
    let preset_idx = select_menu(stdout, &labels)?;
    print_line(stdout, "")?;

    Ok(all_presets[preset_idx].clone())
}

fn prompt_custom_provider(
    stdout: &mut io::Stdout,
) -> Result<(String, String, String, String, String)> {
    let provider_name = prompt_text(stdout, "Provider name", "custom-openai")?;
    print_line(stdout, "")?;

    let protocol_labels = vec![
        format!("OpenAI-compatible  {DIM}(OpenAI, Groq, Together, vLLM){RESET}"),
        format!("Gemini             {DIM}(Google AI Studio / Vertex){RESET}"),
        format!("Anthropic-compatible {DIM}(Claude API){RESET}"),
    ];
    print_line(stdout, &format!("  {BOLD}Endpoint protocol{RESET}"))?;
    print_line(stdout, "")?;
    let protocol_idx = select_menu_default(stdout, &protocol_labels, 0)?;
    print_line(stdout, "")?;

    let (protocol, default_url, default_model) = match protocol_idx {
        1 => (
            "gemini".to_string(),
            "https://generativelanguage.googleapis.com/v1beta".to_string(),
            "gemini-2.5-flash".to_string(),
        ),
        2 => (
            "anthropic".to_string(),
            "https://api.anthropic.com/v1".to_string(),
            "claude-sonnet-4-5-20250514".to_string(),
        ),
        _ => (
            "openai".to_string(),
            "https://api.openai.com/v1".to_string(),
            "gpt-4o-mini".to_string(),
        ),
    };

    let base_url = prompt_text(stdout, "Base URL", &default_url)?;
    let model = prompt_text(stdout, "Model", &default_model)?;
    let secret_key = format!("{}_api_key", provider_name.replace('-', "_"));
    print_line(stdout, "")?;

    Ok((provider_name, protocol, base_url, model, secret_key))
}
