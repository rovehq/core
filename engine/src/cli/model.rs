use std::io::{self, Write};

use anyhow::Result;

use crate::config::{Config, CustomProvider};
use crate::security::secrets::SecretManager;

const CYAN: &str = "\x1b[38;5;39m";
const GREEN: &str = "\x1b[38;5;48m";
const DIM: &str = "\x1b[38;5;240m";
const BOLD: &str = "\x1b[1m";
const YELLOW: &str = "\x1b[38;5;220m";
const RESET: &str = "\x1b[0m";

pub async fn handle_setup() -> Result<()> {
    println!();
    println!("  {CYAN}{BOLD}=== Add LLM Provider ==={RESET}");
    println!();

    let name = prompt_required("Provider name", "e.g. groq, together, local-llama")?;
    let (protocol, default_url, default_model) = prompt_protocol()?;
    let base_url = prompt_value("Base URL", default_url)?;
    let model = prompt_value("Model", default_model)?;
    let secret_key = format!("{}_api_key", name.replace('-', "_"));

    println!();
    print!("  {BOLD}API key{RESET}: ");
    io::stdout().flush()?;
    let mut key = String::new();
    io::stdin().read_line(&mut key)?;
    let key = key.trim();
    if !key.is_empty() {
        let secret_manager = SecretManager::new("rove");
        secret_manager.set_secret(&secret_key, key).await?;
        println!("    {GREEN}Stored in keychain as \"{secret_key}\"{RESET}");
    } else {
        println!("    {DIM}Skipped (no API key set){RESET}");
    }

    println!();
    print!("  Set as default provider? {DIM}[y/N]{RESET}: ");
    io::stdout().flush()?;
    let mut default_choice = String::new();
    io::stdin().read_line(&mut default_choice)?;
    let set_default = default_choice.trim().eq_ignore_ascii_case("y");

    let mut config = Config::load_or_create()?;
    config
        .llm
        .custom_providers
        .retain(|provider| provider.name != name);
    config.llm.custom_providers.push(CustomProvider {
        name: name.clone(),
        protocol: protocol.to_string(),
        base_url,
        model,
        secret_key,
        is_cloud: false,
        no_system_prompt: false,
    });

    if set_default {
        config.llm.default_provider = name.clone();
    }

    if dirs::home_dir().is_none() {
        anyhow::bail!("Cannot determine home directory");
    }
    let config_path = crate::config::paths::rove_home().join("config.toml");
    let toml_string = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, toml_string)?;

    println!();
    println!(
        "  {GREEN}Saved provider \"{BOLD}{name}{RESET}{GREEN}\" to {}{RESET}",
        config_path.display()
    );
    if set_default {
        println!("  {GREEN}Set as default provider{RESET}");
    }
    println!();

    Ok(())
}

pub async fn handle_list() -> Result<()> {
    let config = Config::load_or_create()?;
    let secret_manager = SecretManager::new("rove");

    println!();
    println!("  {CYAN}{BOLD}Configured LLM Providers{RESET}");
    println!("  {DIM}─────────────────────────{RESET}");

    let builtins = [
        (
            "ollama",
            "local",
            &config.llm.ollama.base_url,
            &config.llm.ollama.model,
            "",
        ),
        (
            "openai",
            "openai",
            &config.llm.openai.base_url,
            &config.llm.openai.model,
            "openai_api_key",
        ),
        (
            "anthropic",
            "anthropic",
            &config.llm.anthropic.base_url,
            &config.llm.anthropic.model,
            "anthropic_api_key",
        ),
        (
            "gemini",
            "gemini",
            &config.llm.gemini.base_url,
            &config.llm.gemini.model,
            "gemini_api_key",
        ),
        (
            "nvidia_nim",
            "openai",
            &config.llm.nvidia_nim.base_url,
            &config.llm.nvidia_nim.model,
            "nvidia_nim_api_key",
        ),
    ];

    for (name, protocol, url, model, secret_key) in builtins {
        print_provider(
            name,
            protocol,
            url,
            model,
            secret_key.is_empty() || secret_manager.has_secret(secret_key).await,
            config.llm.default_provider == name,
            false,
        );
    }

    for provider in &config.llm.custom_providers {
        print_provider(
            &provider.name,
            &provider.protocol,
            &provider.base_url,
            &provider.model,
            secret_manager.has_secret(&provider.secret_key).await,
            config.llm.default_provider == provider.name,
            true,
        );
    }

    println!();
    Ok(())
}

fn prompt_required(label: &str, hint: &str) -> Result<String> {
    loop {
        print!("  {BOLD}{label}{RESET} {DIM}({hint}){RESET}: ");
        io::stdout().flush()?;

        let mut value = String::new();
        io::stdin().read_line(&mut value)?;
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Ok(value);
        }

        println!("    {DIM}A name is required.{RESET}");
    }
}

fn prompt_protocol() -> Result<(&'static str, &'static str, &'static str)> {
    println!();
    println!("  {BOLD}Protocol:{RESET}");
    println!("    {GREEN}1.{RESET} OpenAI-compatible  {DIM}(OpenAI, Groq, Together, Ollama, vLLM){RESET}");
    println!("    {GREEN}2.{RESET} Gemini             {DIM}(Google AI Studio / Vertex){RESET}");
    println!("    {GREEN}3.{RESET} Anthropic-compatible {DIM}(Claude API){RESET}");
    print!("  Select {DIM}[1]{RESET}: ");
    io::stdout().flush()?;

    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;

    let protocol = match choice.trim() {
        "2" => (
            "gemini",
            "https://generativelanguage.googleapis.com/v1beta",
            "gemini-2.5-flash",
        ),
        "3" => (
            "anthropic",
            "https://api.anthropic.com/v1",
            "claude-sonnet-4-5-20250514",
        ),
        _ => ("openai", "https://api.openai.com/v1", "gpt-4o-mini"),
    };

    Ok(protocol)
}

fn prompt_value(label: &str, default: &str) -> Result<String> {
    print!("  {BOLD}{label}{RESET} {DIM}[{default}]{RESET}: ");
    io::stdout().flush()?;

    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim();

    if value.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(value.to_string())
    }
}

fn print_provider(
    name: &str,
    protocol: &str,
    url: &str,
    model: &str,
    has_key: bool,
    is_default: bool,
    is_custom: bool,
) {
    let default_badge = if is_default {
        format!(" {YELLOW}* default{RESET}")
    } else {
        String::new()
    };
    let custom_badge = if is_custom {
        format!(" {DIM}(custom){RESET}")
    } else {
        String::new()
    };
    let key_status = if has_key {
        format!("{GREEN}key{RESET}")
    } else {
        format!("{DIM}no key{RESET}")
    };

    println!();
    println!("  {BOLD}{name}{RESET}{default_badge}{custom_badge}");
    println!("    {DIM}protocol:{RESET} {protocol}  {DIM}model:{RESET} {model}  {key_status}");
    println!("    {DIM}url:{RESET} {url}");
}
