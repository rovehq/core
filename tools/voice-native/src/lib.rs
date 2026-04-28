use anyhow::{anyhow, Result};
use sdk::{CoreContext, CoreTool, EngineError, ToolInput, ToolOutput};
use serde::Serialize;
use serde_json::json;
use std::process::Command;

#[derive(Default)]
pub struct VoiceNativeTool {
    #[allow(dead_code)]
    ctx: Option<CoreContext>,
}

#[derive(Serialize)]
struct NativeDeviceRecord {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    default: bool,
    available: bool,
}

impl VoiceNativeTool {
    pub fn new() -> Self {
        Self { ctx: None }
    }
}

impl CoreTool for VoiceNativeTool {
    fn name(&self) -> &str {
        "voice-native"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn start(&mut self, ctx: CoreContext) -> Result<(), EngineError> {
        self.ctx = Some(ctx);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        self.ctx = None;
        Ok(())
    }

    fn handle(&self, input: ToolInput) -> Result<ToolOutput, EngineError> {
        match input.method.as_str() {
            "list_devices" => Ok(ToolOutput::json(json!({
                "devices": [
                    NativeDeviceRecord {
                        id: "system-default-input",
                        name: "System Default Input",
                        kind: "input",
                        default: true,
                        available: true,
                    },
                    NativeDeviceRecord {
                        id: "system-default-output",
                        name: "System Default Output",
                        kind: "output",
                        default: true,
                        available: true,
                    }
                ]
            }))),
            "test_output" => {
                let text = input
                    .param_str("text")
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                let voice = input.param_str_opt("voice");
                let result = speak_text(&text, voice.as_deref())
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(json!({
                    "ok": true,
                    "message": result,
                })))
            }
            "test_input" => Ok(ToolOutput::error(
                "Native microphone capture is not implemented in this build yet.".to_string(),
            )),
            _ => Ok(ToolOutput::error(format!(
                "Unknown voice-native method '{}'",
                input.method
            ))),
        }
    }
}

fn speak_text(text: &str, voice: Option<&str>) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("say");
        if let Some(voice) = voice.filter(|value| !value.trim().is_empty()) {
            command.args(["-v", voice]);
        }
        let status = command.arg(text).status()?;
        if !status.success() {
            return Err(anyhow!("'say' exited with status {}", status));
        }
        return Ok("Spoken through macOS 'say'".to_string());
    }

    #[cfg(target_os = "linux")]
    {
        for candidate in ["spd-say", "espeak"] {
            let status = Command::new(candidate).arg(text).status();
            match status {
                Ok(status) if status.success() => {
                    return Ok(format!("Spoken through '{}'", candidate));
                }
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            }
        }
        return Err(anyhow!(
            "Neither 'spd-say' nor 'espeak' is installed on this system"
        ));
    }

    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "Add-Type -AssemblyName System.Speech; \
             $speak = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             $speak.Speak('{}');",
            text.replace('\'', "''")
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status()?;
        if !status.success() {
            return Err(anyhow!("PowerShell speech exited with status {}", status));
        }
        return Ok("Spoken through Windows SpeechSynthesizer".to_string());
    }

    #[allow(unreachable_code)]
    Err(anyhow!("Voice output is unsupported on this platform"))
}

#[no_mangle]
pub fn create_tool() -> *mut dyn CoreTool {
    Box::into_raw(Box::new(VoiceNativeTool::new()))
}
