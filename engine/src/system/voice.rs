use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use sdk::{
    VoiceAssetStatus, VoiceDeviceKind, VoiceDeviceRecord, VoiceEngineInput,
    VoiceEngineInstallRequest, VoiceEngineKind, VoiceEngineReadiness, VoiceEngineRecord,
    VoiceOutputTestRequest, VoicePolicyControls, VoiceRuntimeStatus, VoiceSurfaceStatus,
    VoiceSurfaceUpdate, VoiceTestResult,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::cli::database_path::{database_path, expand_data_dir};
use crate::cli::extensions::{install_official_system, remove_official_system};
use crate::config::{
    Config, VoiceConfig, VoiceEngineConfig, VoiceEngineKind as ConfigKind, VoicePolicyConfig,
};
use crate::storage::{Database, InstalledPlugin};

pub const VOICE_NATIVE_SYSTEM_ID: &str = "voice-native";

const KNOWN_ENGINES: [VoiceEngineKind; 3] = [
    VoiceEngineKind::NativeOs,
    VoiceEngineKind::LocalWhisper,
    VoiceEngineKind::LocalPiper,
];

pub struct VoiceManager {
    config: Config,
}

impl VoiceManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn status(&self) -> Result<VoiceSurfaceStatus> {
        let database = self.database().await?;
        let runtime_plugin = resolve_voice_runtime_plugin(&database).await?;
        let mut runtime = runtime_status(runtime_plugin.as_ref());
        let devices = if runtime.installed && runtime.enabled {
            match runtime_plugin
                .as_ref()
                .and_then(|plugin| plugin.binary_path.as_deref())
                .map(PathBuf::from)
            {
                Some(artifact) => match list_devices_from_runtime(&artifact) {
                    Ok(devices) => devices,
                    Err(error) => {
                        runtime.warnings.push(format!(
                            "Voice Pack is installed but device discovery failed: {}",
                            error
                        ));
                        Vec::new()
                    }
                },
                None => {
                    runtime.warnings.push(
                        "Voice Pack is installed without a native artifact path.".to_string(),
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        let voice = &self.config.voice;
        let mut warnings = Vec::new();
        warnings.extend(runtime.warnings.clone());

        if !voice.enabled {
            warnings.push(
                "Voice support is disabled. Install and enable the Voice Pack before binding speech input or spoken output."
                    .to_string(),
            );
        }
        if voice.enabled && !runtime.installed {
            warnings.push(
                "Voice support is enabled but the official Voice Pack is not installed yet."
                    .to_string(),
            );
        }
        if voice.enabled && voice.active_input_engine.is_none() {
            warnings.push(
                "No active input engine is selected. Speech input stays unavailable until one is activated."
                    .to_string(),
            );
        }
        if voice.enabled && voice.active_output_engine.is_none() {
            warnings.push(
                "No active output engine is selected. Spoken output stays unavailable until one is activated."
                    .to_string(),
            );
        }
        if voice.policy.allow_remote_audio_input {
            warnings.push(
                "Remote audio input is enabled. Audio capture can cross the local node boundary."
                    .to_string(),
            );
        }
        if voice.policy.allow_remote_audio_output {
            warnings.push(
                "Remote audio output is enabled. Spoken output can cross the local node boundary."
                    .to_string(),
            );
        }
        if voice.policy.persist_transcripts {
            warnings.push(
                "Transcript persistence is enabled. Spoken artifacts can remain in local storage."
                    .to_string(),
            );
        }

        if let Some(device_id) = voice.selected_input_device_id.as_deref() {
            if !devices
                .iter()
                .any(|device| device.kind == VoiceDeviceKind::Input && device.id == device_id)
            {
                warnings.push(format!(
                    "Selected input device '{}' is not currently visible to the Voice Pack.",
                    device_id
                ));
            }
        }
        if let Some(device_id) = voice.selected_output_device_id.as_deref() {
            if !devices
                .iter()
                .any(|device| device.kind == VoiceDeviceKind::Output && device.id == device_id)
            {
                warnings.push(format!(
                    "Selected output device '{}' is not currently visible to the Voice Pack.",
                    device_id
                ));
            }
        }

        let configured = voice
            .engines
            .iter()
            .map(|engine| (kind_from_config(engine.kind), engine))
            .collect::<HashMap<_, _>>();

        let engines = KNOWN_ENGINES
            .into_iter()
            .map(|kind| {
                engine_record(
                    kind,
                    configured.get(&kind).copied(),
                    voice,
                    runtime.installed,
                    runtime.enabled,
                )
            })
            .collect::<Vec<_>>();

        Ok(VoiceSurfaceStatus {
            enabled: voice.enabled,
            runtime,
            active_input_engine: voice.active_input_engine.map(kind_from_config),
            active_output_engine: voice.active_output_engine.map(kind_from_config),
            selected_input_device_id: voice.selected_input_device_id.clone(),
            selected_output_device_id: voice.selected_output_device_id.clone(),
            policy: policy_from_config(&voice.policy),
            devices,
            engines,
            warnings,
        })
    }

    pub async fn replace(&self, update: VoiceSurfaceUpdate) -> Result<VoiceSurfaceStatus> {
        let mut config = Config::load_or_create()?;
        config.voice = config_from_update(&config, update)?;
        config.save()?;
        Self::new(config).status().await
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<VoiceSurfaceStatus> {
        let mut update = update_from_config(&self.config.voice);
        update.enabled = enabled;
        if enabled {
            install_official_system(&self.config, VOICE_NATIVE_SYSTEM_ID, false).await?;
            ensure_engine_present(&mut update.engines, VoiceEngineKind::NativeOs, None)?;
            if update.active_input_engine.is_none() {
                update.active_input_engine = Some(VoiceEngineKind::NativeOs);
            }
            if update.active_output_engine.is_none() {
                update.active_output_engine = Some(VoiceEngineKind::NativeOs);
            }
        }
        self.replace(update).await
    }

    pub async fn set_policy(&self, policy: VoicePolicyControls) -> Result<VoiceSurfaceStatus> {
        let mut update = update_from_config(&self.config.voice);
        update.policy = policy;
        self.replace(update).await
    }

    pub async fn install_engine(
        &self,
        request: VoiceEngineInstallRequest,
    ) -> Result<VoiceSurfaceStatus> {
        install_official_system(&self.config, VOICE_NATIVE_SYSTEM_ID, false).await?;

        let mut update = update_from_config(&self.config.voice);
        let mut engine = VoiceEngineInput {
            kind: request.engine,
            enabled: true,
            model: normalize_optional(request.model),
            voice: normalize_optional(request.voice),
            runtime_path: normalize_optional(request.runtime_path),
            asset_dir: None,
            notes: normalize_optional(request.notes),
        };

        if !matches!(request.engine, VoiceEngineKind::NativeOs) {
            let asset_dir = managed_engine_dir(&self.config, request.engine);
            write_engine_manifest(&asset_dir, request.engine, &engine)?;
            engine.asset_dir = Some(asset_dir.display().to_string());
        }

        ensure_engine_present(&mut update.engines, request.engine, Some(engine))?;
        if request.engine.supports_input() && update.active_input_engine.is_none() {
            update.active_input_engine = Some(request.engine);
        }
        if request.engine.supports_output() && update.active_output_engine.is_none() {
            update.active_output_engine = Some(request.engine);
        }

        self.replace(update).await
    }

    pub async fn uninstall_engine(&self, engine: VoiceEngineKind) -> Result<VoiceSurfaceStatus> {
        let mut update = update_from_config(&self.config.voice);
        update.engines.retain(|existing| existing.kind != engine);

        if matches!(engine, VoiceEngineKind::NativeOs) {
            remove_official_system(&self.config, VOICE_NATIVE_SYSTEM_ID).await?;
        } else if let Some(path) = managed_engine_dir_if_present(&self.config.voice, engine) {
            if path.exists() {
                fs::remove_dir_all(&path).with_context(|| {
                    format!(
                        "Failed to remove managed voice asset dir '{}'",
                        path.display()
                    )
                })?;
            }
        }

        if update.active_input_engine == Some(engine) {
            update.active_input_engine = first_enabled_engine(&update.engines, true);
        }
        if update.active_output_engine == Some(engine) {
            update.active_output_engine = first_enabled_engine(&update.engines, false);
        }

        self.replace(update).await
    }

    pub async fn activate_input(&self, engine: VoiceEngineKind) -> Result<VoiceSurfaceStatus> {
        if !engine.supports_input() {
            return Err(anyhow!(
                "Voice engine '{}' does not support speech input",
                engine.as_str()
            ));
        }
        let mut update = update_from_config(&self.config.voice);
        let current = update
            .engines
            .iter()
            .find(|existing| existing.kind == engine)
            .ok_or_else(|| anyhow!("Voice engine '{}' is not installed", engine.as_str()))?;
        if !current.enabled {
            return Err(anyhow!(
                "Voice engine '{}' is installed but disabled",
                engine.as_str()
            ));
        }
        update.active_input_engine = Some(engine);
        self.replace(update).await
    }

    pub async fn activate_output(&self, engine: VoiceEngineKind) -> Result<VoiceSurfaceStatus> {
        if !engine.supports_output() {
            return Err(anyhow!(
                "Voice engine '{}' does not support spoken output",
                engine.as_str()
            ));
        }
        let mut update = update_from_config(&self.config.voice);
        let current = update
            .engines
            .iter()
            .find(|existing| existing.kind == engine)
            .ok_or_else(|| anyhow!("Voice engine '{}' is not installed", engine.as_str()))?;
        if !current.enabled {
            return Err(anyhow!(
                "Voice engine '{}' is installed but disabled",
                engine.as_str()
            ));
        }
        update.active_output_engine = Some(engine);
        self.replace(update).await
    }

    pub async fn test_input(&self) -> Result<VoiceTestResult> {
        let status = self.status().await?;
        let engine = status
            .active_input_engine
            .ok_or_else(|| anyhow!("No active voice input engine is selected"))?;

        match engine {
            VoiceEngineKind::NativeOs => {
                let plugin = self.voice_runtime_plugin().await?.ok_or_else(|| {
                    anyhow!("Voice Pack is not installed; install native-os first")
                })?;
                let artifact = voice_runtime_artifact(&plugin)?;
                let payload = json!({});
                let response =
                    call_native::<NativeMessageResponse>("test_input", &artifact, &payload)?;
                Ok(VoiceTestResult {
                    ok: response.ok.unwrap_or(true),
                    engine,
                    message: response
                        .message
                        .unwrap_or_else(|| "Native input check completed.".to_string()),
                })
            }
            VoiceEngineKind::LocalWhisper => Err(anyhow!(
                "Managed local Whisper install is recorded, but actual STT execution is not wired in this build yet"
            )),
            VoiceEngineKind::LocalPiper => Err(anyhow!(
                "Voice engine '{}' cannot be used for input",
                engine.as_str()
            )),
        }
    }

    pub async fn test_output(&self, request: VoiceOutputTestRequest) -> Result<VoiceTestResult> {
        let status = self.status().await?;
        let engine = status
            .active_output_engine
            .ok_or_else(|| anyhow!("No active voice output engine is selected"))?;

        match engine {
            VoiceEngineKind::NativeOs => {
                let plugin = self.voice_runtime_plugin().await?.ok_or_else(|| {
                    anyhow!("Voice Pack is not installed; install native-os first")
                })?;
                let artifact = voice_runtime_artifact(&plugin)?;
                let payload = json!({
                    "text": request.text,
                    "voice": request.voice,
                });
                let response =
                    call_native::<NativeMessageResponse>("test_output", &artifact, &payload)?;
                Ok(VoiceTestResult {
                    ok: response.ok.unwrap_or(true),
                    engine,
                    message: response
                        .message
                        .unwrap_or_else(|| "Native output test completed.".to_string()),
                })
            }
            VoiceEngineKind::LocalWhisper => Err(anyhow!(
                "Voice engine '{}' cannot be used for spoken output",
                engine.as_str()
            )),
            VoiceEngineKind::LocalPiper => Err(anyhow!(
                "Managed local Piper install is recorded, but actual self-hosted TTS execution is not wired in this build yet"
            )),
        }
    }

    async fn database(&self) -> Result<Database> {
        Database::new(&database_path(&self.config)).await
    }

    async fn voice_runtime_plugin(&self) -> Result<Option<InstalledPlugin>> {
        let database = self.database().await?;
        resolve_voice_runtime_plugin(&database).await
    }
}

#[derive(Debug, Clone, Serialize)]
struct ManagedEngineManifest {
    engine: String,
    model: Option<String>,
    voice: Option<String>,
    runtime_path: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NativeDeviceListResponse {
    #[serde(default)]
    devices: Vec<NativeDeviceRecord>,
}

#[derive(Debug, Deserialize)]
struct NativeDeviceRecord {
    id: String,
    name: String,
    kind: String,
    #[serde(default)]
    default: bool,
    #[serde(default)]
    available: bool,
}

#[derive(Debug, Deserialize)]
struct NativeMessageResponse {
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    message: Option<String>,
}

async fn resolve_voice_runtime_plugin(database: &Database) -> Result<Option<InstalledPlugin>> {
    let installed = database
        .installed_plugins()
        .list_plugins()
        .await
        .context("Failed to list installed plugins for voice runtime")?;
    Ok(installed.into_iter().find(|plugin| {
        plugin.id == VOICE_NATIVE_SYSTEM_ID || plugin.name == VOICE_NATIVE_SYSTEM_ID
    }))
}

fn runtime_status(plugin: Option<&InstalledPlugin>) -> VoiceRuntimeStatus {
    let mut warnings = Vec::new();
    let (installed, enabled, version, artifact_path) = if let Some(plugin) = plugin {
        if !plugin.enabled {
            warnings.push(
                "Voice Pack is installed but disabled. Enable the official system before relying on local audio devices."
                    .to_string(),
            );
        }
        (
            true,
            plugin.enabled,
            Some(plugin.version.clone()),
            plugin.binary_path.clone(),
        )
    } else {
        warnings.push(
            "Voice Pack is not installed. Install native-os before enabling voice input or spoken output."
                .to_string(),
        );
        (false, false, None, None)
    };

    VoiceRuntimeStatus {
        system_id: VOICE_NATIVE_SYSTEM_ID.to_string(),
        installed,
        enabled,
        version,
        artifact_path,
        warnings,
    }
}

fn engine_record(
    kind: VoiceEngineKind,
    configured: Option<&VoiceEngineConfig>,
    voice: &VoiceConfig,
    runtime_installed: bool,
    runtime_enabled: bool,
) -> VoiceEngineRecord {
    let installed = match kind {
        VoiceEngineKind::NativeOs => runtime_installed,
        _ => configured.is_some(),
    };
    let enabled = configured.map(|engine| engine.enabled).unwrap_or(false);
    let model = configured.and_then(|engine| engine.model.clone());
    let voice_name = configured.and_then(|engine| engine.voice.clone());
    let runtime_path = configured.and_then(|engine| engine.runtime_path.clone());
    let asset_dir = configured.and_then(|engine| engine.asset_dir.clone());
    let notes = configured.and_then(|engine| engine.notes.clone());
    let mut warnings = Vec::new();

    let asset_status = match kind {
        VoiceEngineKind::NativeOs => VoiceAssetStatus::NoneRequired,
        _ => match asset_dir.as_deref() {
            Some(path) if Path::new(path).exists() => VoiceAssetStatus::Managed,
            Some(_) => VoiceAssetStatus::Missing,
            None => VoiceAssetStatus::Missing,
        },
    };

    let readiness = match kind {
        VoiceEngineKind::NativeOs => {
            if !runtime_installed {
                warnings.push(
                    "Install the official Voice Pack to expose local devices and OS-native speech."
                        .to_string(),
                );
                VoiceEngineReadiness::NeedsSetup
            } else if !runtime_enabled {
                warnings.push(
                    "Voice Pack is installed but disabled, so native voice access is not active."
                        .to_string(),
                );
                VoiceEngineReadiness::Warning
            } else {
                VoiceEngineReadiness::Ready
            }
        }
        VoiceEngineKind::LocalWhisper => {
            if !installed {
                warnings.push(
                    "Install local_whisper when you want a self-hosted speech-input engine."
                        .to_string(),
                );
                VoiceEngineReadiness::NeedsSetup
            } else if model.is_none() {
                warnings.push(
                    "Select a Whisper model before activating self-hosted speech input."
                        .to_string(),
                );
                VoiceEngineReadiness::NeedsSetup
            } else if !runtime_installed {
                warnings.push(
                    "Voice Pack is still required to broker local microphone access for self-hosted STT."
                        .to_string(),
                );
                VoiceEngineReadiness::NeedsSetup
            } else {
                warnings.push(
                    "Managed local Whisper state is recorded, but live self-hosted STT execution is not wired in this build yet."
                        .to_string(),
                );
                VoiceEngineReadiness::Warning
            }
        }
        VoiceEngineKind::LocalPiper => {
            if !installed {
                warnings.push(
                    "Install local_piper when you want a self-hosted spoken-output engine."
                        .to_string(),
                );
                VoiceEngineReadiness::NeedsSetup
            } else if voice_name.is_none() {
                warnings.push(
                    "Select a Piper voice before activating self-hosted spoken output.".to_string(),
                );
                VoiceEngineReadiness::NeedsSetup
            } else if !runtime_installed {
                warnings.push(
                    "Voice Pack is still required to broker local speaker output for self-hosted TTS."
                        .to_string(),
                );
                VoiceEngineReadiness::NeedsSetup
            } else {
                warnings.push(
                    "Managed local Piper state is recorded, but live self-hosted TTS execution is not wired in this build yet."
                        .to_string(),
                );
                VoiceEngineReadiness::Warning
            }
        }
    };

    VoiceEngineRecord {
        kind,
        id: kind.as_str().to_string(),
        name: engine_name(kind).to_string(),
        installed,
        enabled,
        supports_input: kind.supports_input(),
        supports_output: kind.supports_output(),
        active_input: voice.active_input_engine.map(kind_from_config) == Some(kind),
        active_output: voice.active_output_engine.map(kind_from_config) == Some(kind),
        asset_status,
        readiness,
        model,
        voice: voice_name,
        runtime_path,
        asset_dir,
        notes,
        approval_required_for_input: kind.supports_input() && voice.policy.require_approval_for_stt,
        approval_required_for_output: kind.supports_output()
            && voice.policy.require_approval_for_tts,
        warnings,
    }
}

fn engine_name(kind: VoiceEngineKind) -> &'static str {
    match kind {
        VoiceEngineKind::NativeOs => "Native OS Voice",
        VoiceEngineKind::LocalWhisper => "Local Whisper",
        VoiceEngineKind::LocalPiper => "Local Piper",
    }
}

fn list_devices_from_runtime(artifact: &Path) -> Result<Vec<VoiceDeviceRecord>> {
    let payload = json!({});
    let devices = call_native::<NativeDeviceListResponse>("list_devices", artifact, &payload)?;
    Ok(devices
        .devices
        .into_iter()
        .map(|device| VoiceDeviceRecord {
            id: device.id,
            name: device.name,
            kind: parse_device_kind(&device.kind),
            default: device.default,
            available: device.available,
        })
        .collect())
}

fn parse_device_kind(kind: &str) -> VoiceDeviceKind {
    match kind {
        "output" => VoiceDeviceKind::Output,
        _ => VoiceDeviceKind::Input,
    }
}

fn policy_from_config(policy: &VoicePolicyConfig) -> VoicePolicyControls {
    VoicePolicyControls {
        require_approval_for_tts: policy.require_approval_for_tts,
        require_approval_for_stt: policy.require_approval_for_stt,
        allow_remote_audio_input: policy.allow_remote_audio_input,
        allow_remote_audio_output: policy.allow_remote_audio_output,
        persist_transcripts: policy.persist_transcripts,
    }
}

fn update_from_config(config: &VoiceConfig) -> VoiceSurfaceUpdate {
    VoiceSurfaceUpdate {
        enabled: config.enabled,
        active_input_engine: config.active_input_engine.map(kind_from_config),
        active_output_engine: config.active_output_engine.map(kind_from_config),
        selected_input_device_id: config.selected_input_device_id.clone(),
        selected_output_device_id: config.selected_output_device_id.clone(),
        policy: policy_from_config(&config.policy),
        engines: config
            .engines
            .iter()
            .map(|engine| VoiceEngineInput {
                kind: kind_from_config(engine.kind),
                enabled: engine.enabled,
                model: engine.model.clone(),
                voice: engine.voice.clone(),
                runtime_path: engine.runtime_path.clone(),
                asset_dir: engine.asset_dir.clone(),
                notes: engine.notes.clone(),
            })
            .collect(),
    }
}

fn config_from_update(base: &Config, update: VoiceSurfaceUpdate) -> Result<VoiceConfig> {
    Ok(VoiceConfig {
        enabled: update.enabled,
        active_input_engine: update.active_input_engine.map(kind_to_config),
        active_output_engine: update.active_output_engine.map(kind_to_config),
        selected_input_device_id: normalize_optional(update.selected_input_device_id),
        selected_output_device_id: normalize_optional(update.selected_output_device_id),
        policy: VoicePolicyConfig {
            require_approval_for_tts: update.policy.require_approval_for_tts,
            require_approval_for_stt: update.policy.require_approval_for_stt,
            allow_remote_audio_input: update.policy.allow_remote_audio_input,
            allow_remote_audio_output: update.policy.allow_remote_audio_output,
            persist_transcripts: update.policy.persist_transcripts,
        },
        engines: update
            .engines
            .into_iter()
            .map(|engine| {
                let kind = kind_to_config(engine.kind);
                Ok(VoiceEngineConfig {
                    kind,
                    enabled: engine.enabled,
                    model: normalize_optional(engine.model),
                    voice: normalize_optional(engine.voice),
                    runtime_path: normalize_optional(engine.runtime_path),
                    asset_dir: normalize_optional(engine.asset_dir)
                        .or_else(|| default_asset_dir_for_engine(base, engine.kind)),
                    notes: normalize_optional(engine.notes),
                })
            })
            .collect::<Result<Vec<_>>>()?,
    })
}

fn default_asset_dir_for_engine(base: &Config, kind: VoiceEngineKind) -> Option<String> {
    (!matches!(kind, VoiceEngineKind::NativeOs))
        .then(|| managed_engine_dir(base, kind).display().to_string())
}

fn kind_from_config(kind: ConfigKind) -> VoiceEngineKind {
    match kind {
        ConfigKind::NativeOs => VoiceEngineKind::NativeOs,
        ConfigKind::LocalWhisper => VoiceEngineKind::LocalWhisper,
        ConfigKind::LocalPiper => VoiceEngineKind::LocalPiper,
    }
}

fn kind_to_config(kind: VoiceEngineKind) -> ConfigKind {
    match kind {
        VoiceEngineKind::NativeOs => ConfigKind::NativeOs,
        VoiceEngineKind::LocalWhisper => ConfigKind::LocalWhisper,
        VoiceEngineKind::LocalPiper => ConfigKind::LocalPiper,
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn ensure_engine_present(
    engines: &mut Vec<VoiceEngineInput>,
    kind: VoiceEngineKind,
    replacement: Option<VoiceEngineInput>,
) -> Result<()> {
    if let Some(existing) = engines.iter_mut().find(|existing| existing.kind == kind) {
        if let Some(replacement) = replacement {
            *existing = replacement;
        } else {
            existing.enabled = true;
        }
        return Ok(());
    }

    engines.push(replacement.unwrap_or(VoiceEngineInput {
        kind,
        enabled: true,
        model: None,
        voice: None,
        runtime_path: None,
        asset_dir: None,
        notes: None,
    }));
    Ok(())
}

fn first_enabled_engine(engines: &[VoiceEngineInput], input: bool) -> Option<VoiceEngineKind> {
    engines.iter().find_map(|engine| {
        if !engine.enabled {
            return None;
        }
        if input && engine.kind.supports_input() {
            return Some(engine.kind);
        }
        if !input && engine.kind.supports_output() {
            return Some(engine.kind);
        }
        None
    })
}

fn managed_engine_dir(config: &Config, engine: VoiceEngineKind) -> PathBuf {
    expand_data_dir(&config.core.data_dir)
        .join("voice")
        .join(engine.as_str())
}

fn managed_engine_dir_if_present(config: &VoiceConfig, engine: VoiceEngineKind) -> Option<PathBuf> {
    config
        .engines
        .iter()
        .find(|existing| kind_from_config(existing.kind) == engine)
        .and_then(|existing| existing.asset_dir.as_deref())
        .map(PathBuf::from)
}

fn write_engine_manifest(
    asset_dir: &Path,
    engine: VoiceEngineKind,
    input: &VoiceEngineInput,
) -> Result<()> {
    fs::create_dir_all(asset_dir).with_context(|| {
        format!(
            "Failed to create managed voice asset dir '{}'",
            asset_dir.display()
        )
    })?;
    let manifest = ManagedEngineManifest {
        engine: engine.as_str().to_string(),
        model: input.model.clone(),
        voice: input.voice.clone(),
        runtime_path: input.runtime_path.clone(),
        notes: input.notes.clone(),
    };
    let manifest_path = asset_dir.join("engine.json");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)
            .context("Failed to serialize managed voice engine manifest")?,
    )
    .with_context(|| format!("Failed to write '{}'", manifest_path.display()))?;
    Ok(())
}

fn voice_runtime_artifact(plugin: &InstalledPlugin) -> Result<PathBuf> {
    plugin
        .binary_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("Voice Pack is installed without a native artifact path"))
}

fn call_native<T: DeserializeOwned>(
    method: &str,
    artifact: &Path,
    payload: &serde_json::Value,
) -> Result<T> {
    let bytes = crate::cli::plugins::call_native_tool(artifact, method, payload)
        .with_context(|| format!("Failed to call '{}' on '{}'", method, artifact.display()))?;
    serde_json::from_slice::<T>(&bytes)
        .with_context(|| format!("Failed to decode '{}' response", method))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_os_surfaces_install_warning_without_runtime() {
        let config = Config::default();
        let voice = &config.voice;

        let record = engine_record(VoiceEngineKind::NativeOs, None, voice, false, false);
        assert_eq!(record.readiness, VoiceEngineReadiness::NeedsSetup);
        assert!(!record.warnings.is_empty());
    }

    #[test]
    fn local_whisper_warns_when_model_is_present_but_runtime_is_not_wired() {
        let mut config = Config::default();
        config.voice.engines = vec![VoiceEngineConfig {
            kind: ConfigKind::LocalWhisper,
            model: Some("tiny".to_string()),
            asset_dir: Some("/tmp/local-whisper".to_string()),
            ..Default::default()
        }];
        let voice = &config.voice;

        let record = engine_record(
            VoiceEngineKind::LocalWhisper,
            voice.engines.first(),
            voice,
            true,
            true,
        );
        assert_eq!(record.readiness, VoiceEngineReadiness::Warning);
    }
}
