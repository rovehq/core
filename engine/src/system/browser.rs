use anyhow::{anyhow, Result};
use sdk::{
    BrowserApprovalControls, BrowserProfileInput, BrowserProfileMode, BrowserProfileReadiness,
    BrowserProfileRecord, BrowserSurfaceStatus, BrowserSurfaceUpdate,
};

use crate::config::{
    BrowserApprovalConfig, BrowserConfig, BrowserProfileConfig, BrowserProfileMode as ConfigMode,
    Config,
};

pub struct BrowserManager {
    config: Config,
}

impl BrowserManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn status(&self) -> BrowserSurfaceStatus {
        let browser = &self.config.browser;
        let mut warnings = Vec::new();

        if !browser.enabled {
            warnings.push(
                "Browser control is disabled. Enable it before relying on browser-backed agents or workflows."
                    .to_string(),
            );
        }
        if browser.enabled && browser.profiles.is_empty() {
            warnings
                .push("Browser control is enabled but no profiles are configured yet.".to_string());
        }
        if browser.enabled && !browser.profiles.is_empty() && browser.default_profile_id.is_none() {
            warnings.push(
                "No default browser profile is selected. Operators will need to pick a profile explicitly."
                    .to_string(),
            );
        }

        let profiles = browser
            .profiles
            .iter()
            .map(|profile| profile_record(profile, browser))
            .collect::<Vec<_>>();

        if let Some(default_profile_id) = browser.default_profile_id.as_deref() {
            if let Some(default_profile) = profiles
                .iter()
                .find(|profile| profile.id == default_profile_id)
            {
                if !default_profile.enabled {
                    warnings.push(format!(
                        "Default browser profile '{}' is currently disabled.",
                        default_profile.name
                    ));
                }
            }
        }

        BrowserSurfaceStatus {
            enabled: browser.enabled,
            default_profile_id: browser.default_profile_id.clone(),
            controls: controls_from_config(&browser.approvals),
            profiles,
            warnings,
        }
    }

    pub fn replace(&self, update: BrowserSurfaceUpdate) -> Result<BrowserSurfaceStatus> {
        let mut config = Config::load_or_create()?;
        config.browser = config_from_update(update);
        config.save()?;
        Ok(Self::new(config).status())
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<BrowserSurfaceStatus> {
        let mut update = update_from_config(&self.config.browser);
        update.enabled = enabled;
        self.replace(update)
    }

    pub fn set_controls(&self, controls: BrowserApprovalControls) -> Result<BrowserSurfaceStatus> {
        let mut update = update_from_config(&self.config.browser);
        update.controls = controls;
        self.replace(update)
    }

    pub fn upsert_profile(
        &self,
        profile: BrowserProfileInput,
        set_default: bool,
    ) -> Result<BrowserSurfaceStatus> {
        let mut update = update_from_config(&self.config.browser);
        let mut replaced = false;
        for existing in &mut update.profiles {
            if existing.id == profile.id {
                *existing = profile.clone();
                replaced = true;
                break;
            }
        }
        if !replaced {
            update.profiles.push(profile.clone());
        }
        if set_default || update.default_profile_id.is_none() {
            update.default_profile_id = Some(profile.id);
        }
        self.replace(update)
    }

    pub fn remove_profile(&self, id: &str) -> Result<BrowserSurfaceStatus> {
        let mut update = update_from_config(&self.config.browser);
        let original_len = update.profiles.len();
        update.profiles.retain(|profile| profile.id != id);
        if update.profiles.len() == original_len {
            return Err(anyhow!("Browser profile '{}' was not found", id));
        }
        if update.default_profile_id.as_deref() == Some(id) {
            update.default_profile_id = update.profiles.first().map(|profile| profile.id.clone());
        }
        self.replace(update)
    }

    pub fn set_default_profile(&self, id: &str) -> Result<BrowserSurfaceStatus> {
        if !self
            .config
            .browser
            .profiles
            .iter()
            .any(|profile| profile.id == id)
        {
            return Err(anyhow!("Browser profile '{}' was not found", id));
        }
        let mut update = update_from_config(&self.config.browser);
        update.default_profile_id = Some(id.to_string());
        self.replace(update)
    }
}

fn profile_record(profile: &BrowserProfileConfig, browser: &BrowserConfig) -> BrowserProfileRecord {
    let mut warnings = Vec::new();
    let mode = mode_from_config(profile.mode);
    let readiness = match mode {
        BrowserProfileMode::ManagedLocal => BrowserProfileReadiness::Ready,
        BrowserProfileMode::AttachExisting => {
            warnings.push(
                "Attaches to an already-running browser session. Existing tabs, cookies, and signed-in state are in scope."
                    .to_string(),
            );
            BrowserProfileReadiness::Warning
        }
        BrowserProfileMode::RemoteCdp => {
            warnings.push(
                "Uses a remote CDP endpoint outside the local node boundary. Treat the browser host and transport path as part of the trust model."
                    .to_string(),
            );
            BrowserProfileReadiness::Warning
        }
    };

    BrowserProfileRecord {
        id: profile.id.clone(),
        name: profile.name.clone(),
        enabled: profile.enabled,
        mode,
        is_default: browser.default_profile_id.as_deref() == Some(profile.id.as_str()),
        browser: profile.browser.clone(),
        user_data_dir: profile.user_data_dir.clone(),
        startup_url: profile.startup_url.clone(),
        cdp_url: profile.cdp_url.clone(),
        notes: profile.notes.clone(),
        readiness,
        approval_required: approval_required(mode, &browser.approvals),
        warnings,
    }
}

fn approval_required(mode: BrowserProfileMode, controls: &BrowserApprovalConfig) -> bool {
    match mode {
        BrowserProfileMode::ManagedLocal => controls.require_approval_for_managed_launch,
        BrowserProfileMode::AttachExisting => controls.require_approval_for_existing_session_attach,
        BrowserProfileMode::RemoteCdp => controls.require_approval_for_remote_cdp,
    }
}

fn controls_from_config(config: &BrowserApprovalConfig) -> BrowserApprovalControls {
    BrowserApprovalControls {
        require_approval_for_managed_launch: config.require_approval_for_managed_launch,
        require_approval_for_existing_session_attach: config
            .require_approval_for_existing_session_attach,
        require_approval_for_remote_cdp: config.require_approval_for_remote_cdp,
    }
}

fn update_from_config(config: &BrowserConfig) -> BrowserSurfaceUpdate {
    BrowserSurfaceUpdate {
        enabled: config.enabled,
        default_profile_id: config.default_profile_id.clone(),
        controls: controls_from_config(&config.approvals),
        profiles: config
            .profiles
            .iter()
            .map(|profile| BrowserProfileInput {
                id: profile.id.clone(),
                name: profile.name.clone(),
                enabled: profile.enabled,
                mode: mode_from_config(profile.mode),
                browser: profile.browser.clone(),
                user_data_dir: profile.user_data_dir.clone(),
                startup_url: profile.startup_url.clone(),
                cdp_url: profile.cdp_url.clone(),
                notes: profile.notes.clone(),
            })
            .collect(),
    }
}

fn config_from_update(update: BrowserSurfaceUpdate) -> BrowserConfig {
    BrowserConfig {
        enabled: update.enabled,
        default_profile_id: normalize_optional(update.default_profile_id),
        approvals: BrowserApprovalConfig {
            require_approval_for_managed_launch: update
                .controls
                .require_approval_for_managed_launch,
            require_approval_for_existing_session_attach: update
                .controls
                .require_approval_for_existing_session_attach,
            require_approval_for_remote_cdp: update.controls.require_approval_for_remote_cdp,
        },
        profiles: update
            .profiles
            .into_iter()
            .map(|profile| BrowserProfileConfig {
                id: profile.id.trim().to_string(),
                name: profile.name.trim().to_string(),
                enabled: profile.enabled,
                mode: mode_to_config(profile.mode),
                browser: normalize_optional(profile.browser),
                user_data_dir: normalize_optional(profile.user_data_dir),
                startup_url: normalize_optional(profile.startup_url),
                cdp_url: normalize_optional(profile.cdp_url),
                notes: normalize_optional(profile.notes),
            })
            .collect(),
    }
}

fn mode_from_config(mode: ConfigMode) -> BrowserProfileMode {
    match mode {
        ConfigMode::ManagedLocal => BrowserProfileMode::ManagedLocal,
        ConfigMode::AttachExisting => BrowserProfileMode::AttachExisting,
        ConfigMode::RemoteCdp => BrowserProfileMode::RemoteCdp,
    }
}

fn mode_to_config(mode: BrowserProfileMode) -> ConfigMode {
    match mode {
        BrowserProfileMode::ManagedLocal => ConfigMode::ManagedLocal,
        BrowserProfileMode::AttachExisting => ConfigMode::AttachExisting,
        BrowserProfileMode::RemoteCdp => ConfigMode::RemoteCdp,
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_and_remote_profiles_surface_operator_warnings() {
        let mut config = Config::default();
        config.browser.enabled = true;
        config.browser.default_profile_id = Some("managed".to_string());
        config.browser.profiles = vec![
            BrowserProfileConfig {
                id: "managed".to_string(),
                name: "Managed".to_string(),
                mode: ConfigMode::ManagedLocal,
                ..Default::default()
            },
            BrowserProfileConfig {
                id: "attach".to_string(),
                name: "Attach".to_string(),
                mode: ConfigMode::AttachExisting,
                cdp_url: Some("http://127.0.0.1:9222".to_string()),
                ..Default::default()
            },
            BrowserProfileConfig {
                id: "remote".to_string(),
                name: "Remote".to_string(),
                mode: ConfigMode::RemoteCdp,
                cdp_url: Some("wss://browser.example/ws".to_string()),
                ..Default::default()
            },
        ];

        let status = BrowserManager::new(config).status();
        assert_eq!(status.profiles.len(), 3);
        assert_eq!(status.profiles[0].readiness, BrowserProfileReadiness::Ready);
        assert_eq!(
            status.profiles[1].readiness,
            BrowserProfileReadiness::Warning
        );
        assert_eq!(
            status.profiles[2].readiness,
            BrowserProfileReadiness::Warning
        );
        assert!(!status.profiles[1].warnings.is_empty());
        assert!(!status.profiles[2].warnings.is_empty());
    }
}
