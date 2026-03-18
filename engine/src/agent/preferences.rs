use crate::llm::router::LLMRouter;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Manages user preferences persistently in ~/.rove/preferences.toml
#[derive(Clone)]
pub struct PreferencesManager {
    path: PathBuf,
    preferences: Arc<RwLock<PreferencesData>>,
    llm_router: Arc<LLMRouter>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreferencesData {
    pub rules: Vec<String>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, toml::Value>,
}

impl PreferencesManager {
    /// Create a new PreferencesManager
    pub fn new(llm_router: Arc<LLMRouter>) -> Self {
        let path = preferences_path();

        let mut manager = Self {
            path,
            preferences: Arc::new(RwLock::new(PreferencesData::default())),
            llm_router,
        };

        // Attempt initial load synchronously to have them ready immediately
        if let Err(e) = manager.load_sync() {
            debug!(
                "Preferences not loaded or missing (this is normal on first run): {}",
                e
            );
        }

        manager
    }

    /// Load preferences from disk synchronously (used during initialization)
    fn load_sync(&mut self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.path)?;
        let data: PreferencesData = toml::from_str(&content)?;

        if let Ok(mut prefs) = self.preferences.try_write() {
            *prefs = data;
        }

        Ok(())
    }

    /// Load preferences from disk asynchronously
    pub async fn load(&self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }

        let path_clone = self.path.clone();
        let content = tokio::task::spawn_blocking(move || fs::read_to_string(path_clone)).await??;

        let data: PreferencesData = toml::from_str(&content)?;
        let mut prefs = self.preferences.write().await;
        *prefs = data;

        Ok(())
    }

    /// Save current preferences to disk
    pub async fn save(&self) -> Result<()> {
        let prefs = self.preferences.read().await;
        let content = toml::to_string_pretty(&*prefs)?;

        let path_clone = self.path.clone();

        tokio::task::spawn_blocking(move || {
            if let Some(parent) = path_clone.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(path_clone, content)
        })
        .await??;

        Ok(())
    }

    /// Get the preferences formatted as an XML-like context block for the LLM
    pub async fn get_context_block(&self) -> String {
        let prefs = self.preferences.read().await;
        if prefs.rules.is_empty() {
            return String::new();
        }

        let mut block = String::from(
            "\n<user_preferences>\nThese are remembered user preferences and facts from prior interactions. Treat them as authoritative. If the user asks about one of these items, answer from this block instead of saying you do not know.\n",
        );
        for rule in &prefs.rules {
            block.push_str(&format!("- {}\n", rule));
        }
        block.push_str("</user_preferences>\n");

        block
    }

    /// Call the LLM to extract new preferences or corrections from an interaction.
    /// Returns true if rules were added.
    pub async fn extract_and_update(&self, input: &str, output: &str) -> Result<bool> {
        if let Some(rule) = verification_preference(input) {
            return self.add_rules(vec![rule]).await;
        }

        let prompt = format!(
            "Analyze the following user input and agent output.\n\
            Extract any explicit user preferences, rules, or corrections the user wants the agent to remember for future interactions.\n\
            Return ONLY a JSON array of strings, where each string is a distinct rule/preference. If none are found, return an empty array `[]`.\n\n\
            USER INPUT:\n{}\n\n\
            AGENT OUTPUT:\n{}",
            input, output
        );

        let messages = vec![crate::llm::Message::user(&prompt)];

        let (response, _provider) = self.llm_router.call(&messages).await?;

        let content = match response {
            crate::llm::LLMResponse::FinalAnswer(ans) => ans.content,
            crate::llm::LLMResponse::ToolCall(_) => {
                return Ok(false); // Tool calls are not expected here
            }
        };

        // Attempt to parse JSON
        let content = content.trim();
        let content = if content.starts_with("```json") {
            content
                .trim_start_matches("```json")
                .trim_end_matches("```")
                .trim()
        } else if content.starts_with("```") {
            content
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim()
        } else {
            content
        };

        match serde_json::from_str::<Vec<String>>(content) {
            Ok(new_rules) => self.add_rules(new_rules).await,
            Err(e) => {
                debug!(
                    "Failed to parse LLM extraction as JSON: {}. Content: {}",
                    e, content
                );
                Ok(false)
            }
        }
    }

    async fn add_rules(&self, new_rules: Vec<String>) -> Result<bool> {
        if new_rules.is_empty() {
            return Ok(false);
        }

        info!(
            "Extracted new preferences from interaction: {:?}",
            new_rules
        );

        let mut rules_added = false;
        let mut prefs = self.preferences.write().await;

        for rule in new_rules {
            if !prefs.rules.contains(&rule) {
                prefs.rules.push(rule);
                rules_added = true;
            }
        }

        drop(prefs);

        if rules_added {
            self.save()
                .await
                .context("Failed to save updated preferences")?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn verification_preference(input: &str) -> Option<String> {
    let normalized = input.to_ascii_lowercase();
    if normalized.contains("cargo test before saying done") {
        return Some("Run cargo test before saying done.".to_string());
    }
    if normalized.contains("test it")
        || normalized.contains("run tests")
        || normalized.contains("cargo test")
    {
        return Some("Run cargo test before completion.".to_string());
    }
    None
}

fn preferences_path() -> PathBuf {
    if let Some(data_dir) = std::env::var_os("ROVE_DATA_DIR").filter(|value| !value.is_empty()) {
        let data_dir = PathBuf::from(data_dir);
        if let Some(parent) = data_dir.parent() {
            return parent.join("preferences.toml");
        }
        return data_dir.join("preferences.toml");
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rove")
        .join("preferences.toml")
}
