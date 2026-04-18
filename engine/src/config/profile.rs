use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use sdk::errors::EngineError;

use super::{Config, LoadoutConfig, ResolvedLoadout};

pub type ProfilesConfig = BTreeMap<String, ProfileConfig>;
pub type LoadoutsConfig = BTreeMap<String, LoadoutConfig>;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProfileConfig {
    pub loadout: String,
    #[serde(default)]
    pub brain_profile: Option<String>,
    #[serde(default)]
    pub browser_profile: Option<String>,
    #[serde(default)]
    pub approval_profile: Option<String>,
    #[serde(default)]
    pub tool_overrides: BTreeMap<String, String>,
}

impl Config {
    pub fn resolved_loadout(&self) -> Result<ResolvedLoadout, EngineError> {
        let Some((profile_name, profile)) = self.selected_profile_entry() else {
            return Ok(ResolvedLoadout::compatibility_default());
        };

        let Some(loadout) = self.loadouts.get(profile.loadout.as_str()) else {
            return Err(EngineError::Config(format!(
                "Profile '{}' references unknown loadout '{}'",
                profile_name, profile.loadout
            )));
        };

        Ok(ResolvedLoadout {
            profile_name,
            loadout_name: profile.loadout.clone(),
            builtins: normalize_entries(&loadout.builtins),
            drivers: Some(normalize_entries(&loadout.drivers)),
            plugins: Some(normalize_entries(&loadout.plugins)),
            browser_profile: profile.browser_profile.clone(),
            brain_profile: profile.brain_profile.clone(),
            approval_profile: profile.approval_profile.clone(),
        })
    }

    pub fn selected_profile_name(&self) -> Option<String> {
        self.selected_profile_entry().map(|(name, _)| name)
    }

    fn selected_profile_entry(&self) -> Option<(String, &ProfileConfig)> {
        if self.profiles.is_empty() {
            return None;
        }

        if let Some(active_profile) = self.active_profile.as_deref() {
            return self
                .profiles
                .get_key_value(active_profile)
                .map(|(name, profile)| (name.clone(), profile));
        }

        let daemon_profile = self.daemon.profile.as_str();
        if let Some((name, profile)) = self.profiles.get_key_value(daemon_profile) {
            return Some((name.clone(), profile));
        }

        if let Some((name, profile)) = self.profiles.get_key_value("default") {
            return Some((name.clone(), profile));
        }

        if self.profiles.len() == 1 {
            return self
                .profiles
                .iter()
                .next()
                .map(|(name, profile)| (name.clone(), profile));
        }

        None
    }

    pub(super) fn validate_profiles_and_loadouts(&mut self) -> Result<(), EngineError> {
        if let Some(active_profile) = self.active_profile.as_deref() {
            if !self.profiles.contains_key(active_profile) {
                return Err(EngineError::Config(format!(
                    "active_profile '{}' does not exist under [profiles]",
                    active_profile
                )));
            }
        }

        for (profile_name, profile) in &self.profiles {
            if profile.loadout.trim().is_empty() {
                return Err(EngineError::Config(format!(
                    "Profile '{}' must reference a loadout",
                    profile_name
                )));
            }

            if !self.loadouts.contains_key(profile.loadout.as_str()) {
                return Err(EngineError::Config(format!(
                    "Profile '{}' references unknown loadout '{}'",
                    profile_name, profile.loadout
                )));
            }
        }

        if !self.profiles.is_empty() && self.selected_profile_entry().is_none() {
            return Err(EngineError::Config(
                "No active extension profile could be resolved. Set active_profile or define a [profiles.default] or [profiles.<daemon-profile>] entry.".to_string(),
            ));
        }

        for (loadout_name, loadout) in &mut self.loadouts {
            normalize_entry_list(&mut loadout.builtins)?;
            normalize_entry_list(&mut loadout.drivers)?;
            normalize_entry_list(&mut loadout.plugins)?;

            for builtin in &loadout.builtins {
                if !matches!(builtin.as_str(), "filesystem" | "terminal" | "vision") {
                    return Err(EngineError::Config(format!(
                        "Loadout '{}' references unknown builtin '{}'",
                        loadout_name, builtin
                    )));
                }
            }
        }

        Ok(())
    }
}

fn normalize_entry_list(entries: &mut Vec<String>) -> Result<(), EngineError> {
    let mut normalized = BTreeSet::new();
    for entry in entries.drain(..) {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return Err(EngineError::Config(
                "Profile/loadout entries must not be empty".to_string(),
            ));
        }
        normalized.insert(trimmed.to_string());
    }
    *entries = normalized.into_iter().collect();
    Ok(())
}

fn normalize_entries(entries: &[String]) -> BTreeSet<String> {
    entries
        .iter()
        .map(|entry| entry.trim().to_string())
        .collect()
}
