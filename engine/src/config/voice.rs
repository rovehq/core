use serde::{Deserialize, Serialize};

use super::defaults::{default_false, default_true};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceEngineKind {
    #[default]
    NativeOs,
    LocalWhisper,
    LocalPiper,
}

impl VoiceEngineKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NativeOs => "native_os",
            Self::LocalWhisper => "local_whisper",
            Self::LocalPiper => "local_piper",
        }
    }

    pub fn supports_input(&self) -> bool {
        matches!(self, Self::NativeOs | Self::LocalWhisper)
    }

    pub fn supports_output(&self) -> bool {
        matches!(self, Self::NativeOs | Self::LocalPiper)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoicePolicyConfig {
    #[serde(default = "default_true")]
    pub require_approval_for_tts: bool,
    #[serde(default = "default_true")]
    pub require_approval_for_stt: bool,
    #[serde(default = "default_false")]
    pub allow_remote_audio_input: bool,
    #[serde(default = "default_false")]
    pub allow_remote_audio_output: bool,
    #[serde(default = "default_false")]
    pub persist_transcripts: bool,
}

impl Default for VoicePolicyConfig {
    fn default() -> Self {
        Self {
            require_approval_for_tts: true,
            require_approval_for_stt: true,
            allow_remote_audio_input: false,
            allow_remote_audio_output: false,
            persist_transcripts: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceEngineConfig {
    #[serde(default)]
    pub kind: VoiceEngineKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub runtime_path: Option<String>,
    #[serde(default)]
    pub asset_dir: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl Default for VoiceEngineConfig {
    fn default() -> Self {
        Self {
            kind: VoiceEngineKind::NativeOs,
            enabled: true,
            model: None,
            voice: None,
            runtime_path: None,
            asset_dir: None,
            notes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub active_input_engine: Option<VoiceEngineKind>,
    #[serde(default)]
    pub active_output_engine: Option<VoiceEngineKind>,
    #[serde(default)]
    pub selected_input_device_id: Option<String>,
    #[serde(default)]
    pub selected_output_device_id: Option<String>,
    #[serde(default)]
    pub policy: VoicePolicyConfig,
    #[serde(default)]
    pub engines: Vec<VoiceEngineConfig>,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            active_input_engine: None,
            active_output_engine: None,
            selected_input_device_id: None,
            selected_output_device_id: None,
            policy: VoicePolicyConfig::default(),
            engines: Vec::new(),
        }
    }
}
