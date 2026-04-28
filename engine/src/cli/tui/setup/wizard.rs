use anyhow::Result;
use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};
use std::io::{self, Write};

use super::menu::select_menu_default;
use super::preset::{presets, ModelPreset};
use super::prompt::{print_line, prompt_secret, prompt_text_with_nav, NavigationAction};
use super::result::SetupResult;
use super::{BOLD, CYAN, DIM, RESET};

#[derive(Debug, Clone)]
struct WizardState {
    workspace: String,
    preset: Option<ModelPreset>,
    provider_name: String,
    protocol: String,
    base_url: String,
    model: String,
    secret_key: String,
    api_key: String,
    max_risk_tier: u8,
    daemon_password: String,
    password_confirmed: bool,
}

impl Default for WizardState {
    fn default() -> Self {
        Self {
            workspace: "~/projects".to_string(),
            preset: None,
            provider_name: String::new(),
            protocol: String::new(),
            base_url: String::new(),
            model: String::new(),
            secret_key: String::new(),
            api_key: String::new(),
            max_risk_tier: 2,
            daemon_password: String::new(),
            password_confirmed: false,
        }
    }
}

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
    let mut state = WizardState::default();
    let mut step = 0;

    loop {
        execute!(
            stdout,
            terminal::Clear(ClearType::All),
            cursor::MoveTo(0, 0)
        )?;

        print_header(&mut stdout, step)?;

        let action = match step {
            0 => step_workspace(&mut stdout, &mut state)?,
            1 => step_preset(&mut stdout, &mut state)?,
            2 => step_provider_details(&mut stdout, &mut state)?,
            3 => step_api_key(&mut stdout, &mut state)?,
            4 => step_risk_tier(&mut stdout, &mut state)?,
            5 => step_password(&mut stdout, &mut state)?,
            6 => {
                // Final step - show summary and confirm
                return Ok(build_result(&state));
            }
            _ => NavigationAction::Quit,
        };

        match action {
            NavigationAction::Next => step += 1,
            NavigationAction::Back => {
                step = step.saturating_sub(1);
            }
            NavigationAction::Quit => {
                print_line(&mut stdout, "")?;
                print_line(&mut stdout, "  Setup cancelled.")?;
                print_line(&mut stdout, "")?;
                std::process::exit(0);
            }
        }
    }
}

fn print_header(stdout: &mut io::Stdout, step: usize) -> Result<()> {
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
    print_line(
        stdout,
        &format!(
            "  {DIM}Step {}/6 • Use ← → or Tab/Shift+Tab to navigate • Ctrl+C to quit{RESET}",
            step + 1
        ),
    )?;
    print_line(stdout, "")?;
    Ok(())
}

fn step_workspace(stdout: &mut io::Stdout, state: &mut WizardState) -> Result<NavigationAction> {
    print_line(stdout, &format!("  {BOLD}Workspace Directory{RESET}"))?;
    print_line(stdout, "")?;

    match prompt_text_with_nav(stdout, "Workspace directory", &state.workspace)? {
        (Some(value), nav) => {
            state.workspace = value;
            Ok(nav)
        }
        (None, nav) => Ok(nav),
    }
}

fn step_preset(stdout: &mut io::Stdout, state: &mut WizardState) -> Result<NavigationAction> {
    let all_presets = presets();
    let labels = all_presets
        .iter()
        .map(|preset| format!("{}  {DIM}{}{RESET}", preset.label, preset.description))
        .collect::<Vec<_>>();

    print_line(stdout, &format!("  {BOLD}Quick Model Setup{RESET}"))?;
    print_line(stdout, "")?;

    let default_idx = state
        .preset
        .as_ref()
        .and_then(|p| {
            all_presets
                .iter()
                .position(|preset| preset.label == p.label)
        })
        .unwrap_or(0);

    let preset_idx = select_menu_default(stdout, &labels, default_idx)?;
    print_line(stdout, "")?;

    state.preset = Some(all_presets[preset_idx].clone());

    // Update state with preset values
    let preset = &all_presets[preset_idx];
    state.provider_name = preset.provider_name.clone();
    state.protocol = preset.protocol.clone();
    state.base_url = preset.base_url.clone();
    state.model = preset.model.clone();
    state.secret_key = preset.secret_key.clone();

    Ok(NavigationAction::Next)
}

fn step_provider_details(
    stdout: &mut io::Stdout,
    state: &mut WizardState,
) -> Result<NavigationAction> {
    let preset = state.preset.as_ref().unwrap();

    // Skip this step if not custom provider
    if preset.provider_name != "custom" {
        return Ok(NavigationAction::Next);
    }

    print_line(
        stdout,
        &format!("  {BOLD}Custom Provider Configuration{RESET}"),
    )?;
    print_line(stdout, "")?;

    // Provider name
    match prompt_text_with_nav(stdout, "Provider name", &state.provider_name)? {
        (Some(value), NavigationAction::Next) => state.provider_name = value,
        (_, nav @ NavigationAction::Back) => return Ok(nav),
        (_, nav @ NavigationAction::Quit) => return Ok(nav),
        _ => {}
    }
    print_line(stdout, "")?;

    // Protocol selection
    let protocol_labels = vec![
        format!("OpenAI-compatible  {DIM}(OpenAI, Groq, Together, vLLM){RESET}"),
        format!("Gemini             {DIM}(Google AI Studio / Vertex){RESET}"),
        format!("Anthropic-compatible {DIM}(Claude API){RESET}"),
    ];
    print_line(stdout, &format!("  {BOLD}Endpoint protocol{RESET}"))?;
    print_line(stdout, "")?;

    let protocol_idx = match state.protocol.as_str() {
        "gemini" => 1,
        "anthropic" => 2,
        _ => 0,
    };
    let selected_protocol = select_menu_default(stdout, &protocol_labels, protocol_idx)?;
    print_line(stdout, "")?;

    let (protocol, default_url, default_model) = match selected_protocol {
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

    state.protocol = protocol;

    // Base URL
    let current_url = if state.base_url.is_empty() {
        &default_url
    } else {
        &state.base_url
    };
    match prompt_text_with_nav(stdout, "Base URL", current_url)? {
        (Some(value), NavigationAction::Next) => state.base_url = value,
        (_, nav @ NavigationAction::Back) => return Ok(nav),
        (_, nav @ NavigationAction::Quit) => return Ok(nav),
        _ => {}
    }

    // Model
    let current_model = if state.model.is_empty() {
        &default_model
    } else {
        &state.model
    };
    match prompt_text_with_nav(stdout, "Model", current_model)? {
        (Some(value), NavigationAction::Next) => state.model = value,
        (_, nav @ NavigationAction::Back) => return Ok(nav),
        (_, nav @ NavigationAction::Quit) => return Ok(nav),
        _ => {}
    }

    state.secret_key = format!("{}_api_key", state.provider_name.replace('-', "_"));
    print_line(stdout, "")?;

    Ok(NavigationAction::Next)
}

fn step_api_key(stdout: &mut io::Stdout, state: &mut WizardState) -> Result<NavigationAction> {
    let preset = state.preset.as_ref().unwrap();
    let skipped = preset.provider_name.is_empty();

    if skipped || !preset.needs_api_key {
        return Ok(NavigationAction::Next);
    }

    print_line(stdout, &format!("  {BOLD}API Key{RESET}"))?;
    print_line(stdout, "")?;

    match prompt_text_with_nav(stdout, "API key", &state.api_key)? {
        (Some(value), NavigationAction::Next) => {
            state.api_key = value;
            if !state.api_key.is_empty() {
                print_line(stdout, "  Key captured")?;
            }
            print_line(stdout, "")?;
            Ok(NavigationAction::Next)
        }
        (_, nav) => Ok(nav),
    }
}

fn step_risk_tier(stdout: &mut io::Stdout, state: &mut WizardState) -> Result<NavigationAction> {
    let risk_labels = vec![
        format!("Tier 0  {DIM}Read-only operations only{RESET}"),
        format!("Tier 1  {DIM}Allow local modifications with confirmation{RESET}"),
        format!("Tier 2  {DIM}Allow all operations with confirmation{RESET}"),
    ];
    print_line(stdout, &format!("  {BOLD}Maximum Risk Tier{RESET}"))?;
    print_line(stdout, "")?;

    let risk_idx = select_menu_default(stdout, &risk_labels, state.max_risk_tier as usize)?;
    state.max_risk_tier = risk_idx as u8;
    print_line(stdout, "")?;

    Ok(NavigationAction::Next)
}

fn step_password(stdout: &mut io::Stdout, state: &mut WizardState) -> Result<NavigationAction> {
    print_line(stdout, &format!("  {BOLD}Daemon Password{RESET}"))?;
    print_line(stdout, &format!("  {DIM}Minimum 8 characters{RESET}"))?;
    print_line(stdout, "")?;

    loop {
        let password = prompt_secret(stdout, "Daemon password")?;
        if password.trim().len() < 8 {
            print_line(
                stdout,
                "  Password must be at least 8 characters. Try again.",
            )?;
            continue;
        }

        let confirm = prompt_secret(stdout, "Confirm daemon password")?;
        if password != confirm {
            print_line(stdout, "  Passwords did not match. Try again.")?;
            continue;
        }

        state.daemon_password = password;
        state.password_confirmed = true;
        print_line(stdout, "")?;
        break;
    }

    Ok(NavigationAction::Next)
}

fn build_result(state: &WizardState) -> SetupResult {
    let skipped = state
        .preset
        .as_ref()
        .map(|p| p.provider_name.is_empty())
        .unwrap_or(true);

    SetupResult {
        workspace: state.workspace.clone(),
        provider_name: state.provider_name.clone(),
        protocol: state.protocol.clone(),
        base_url: state.base_url.clone(),
        model: state.model.clone(),
        secret_key: state.secret_key.clone(),
        api_key: state.api_key.clone(),
        max_risk_tier: state.max_risk_tier,
        skipped_model: skipped,
        daemon_password: state.daemon_password.clone(),
        recovery_code: None,
        auth_protection: None,
    }
}
