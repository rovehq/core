use anyhow::Result;
use sdk::{
    VoiceEngineInstallRequest, VoiceEngineKind, VoiceInputTestRequest, VoiceOutputTestRequest,
    VoicePolicyControls,
};

use crate::config::Config;
use crate::system::voice::VoiceManager;

use super::commands::{VoiceAction, VoiceDeviceAction, VoiceEngineKindArg, VoicePolicyAction};

pub async fn handle_voice(action: VoiceAction) -> Result<()> {
    let config = Config::load_or_create()?;
    let manager = VoiceManager::new(config);

    match action {
        VoiceAction::Status => print_status(&manager.status().await?),
        VoiceAction::Install {
            engine,
            model,
            voice,
            runtime_path,
            notes,
        } => {
            let request = VoiceEngineInstallRequest {
                engine: parse_engine(engine),
                model,
                voice,
                runtime_path,
                notes,
            };
            print_status(&manager.install_engine(request).await?);
        }
        VoiceAction::Uninstall { engine } => {
            print_status(&manager.uninstall_engine(parse_engine(engine)).await?);
        }
        VoiceAction::Enable => print_status(&manager.set_enabled(true).await?),
        VoiceAction::Disable => print_status(&manager.set_enabled(false).await?),
        VoiceAction::ActivateInput { engine } => {
            print_status(&manager.activate_input(parse_engine(engine)).await?)
        }
        VoiceAction::ActivateOutput { engine } => {
            print_status(&manager.activate_output(parse_engine(engine)).await?)
        }
        VoiceAction::Devices { action } => match action {
            VoiceDeviceAction::List => print_devices(&manager.status().await?),
        },
        VoiceAction::TestInput { audio_path } => {
            let result = manager
                .test_input(VoiceInputTestRequest { audio_path })
                .await?;
            println!(
                "Input test [{}]: {}",
                result.engine.as_str(),
                result.message
            );
        }
        VoiceAction::TestOutput { text, voice } => {
            let result = manager
                .test_output(VoiceOutputTestRequest { text, voice })
                .await?;
            println!(
                "Output test [{}]: {}",
                result.engine.as_str(),
                result.message
            );
        }
        VoiceAction::Policy { action } => match action {
            VoicePolicyAction::Show => print_status(&manager.status().await?),
            VoicePolicyAction::Set {
                require_tts_approval,
                require_stt_approval,
                allow_remote_audio_input,
                allow_remote_audio_output,
                persist_transcripts,
            } => {
                let current = manager.status().await?;
                let policy = VoicePolicyControls {
                    require_approval_for_tts: require_tts_approval
                        .unwrap_or(current.policy.require_approval_for_tts),
                    require_approval_for_stt: require_stt_approval
                        .unwrap_or(current.policy.require_approval_for_stt),
                    allow_remote_audio_input: allow_remote_audio_input
                        .unwrap_or(current.policy.allow_remote_audio_input),
                    allow_remote_audio_output: allow_remote_audio_output
                        .unwrap_or(current.policy.allow_remote_audio_output),
                    persist_transcripts: persist_transcripts
                        .unwrap_or(current.policy.persist_transcripts),
                };
                print_status(&manager.set_policy(policy).await?);
            }
        },
    }

    Ok(())
}

fn parse_engine(kind: VoiceEngineKindArg) -> VoiceEngineKind {
    match kind {
        VoiceEngineKindArg::NativeOs => VoiceEngineKind::NativeOs,
        VoiceEngineKindArg::LocalWhisper => VoiceEngineKind::LocalWhisper,
        VoiceEngineKindArg::LocalPiper => VoiceEngineKind::LocalPiper,
    }
}

fn print_status(status: &sdk::VoiceSurfaceStatus) {
    println!(
        "Voice surface: {}",
        if status.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "Voice Pack: {}{}",
        if status.runtime.installed {
            if status.runtime.enabled {
                "installed/enabled"
            } else {
                "installed/disabled"
            }
        } else {
            "not installed"
        },
        status
            .runtime
            .version
            .as_deref()
            .map(|version| format!(" version={version}"))
            .unwrap_or_default()
    );
    println!(
        "Active input engine: {}",
        status
            .active_input_engine
            .map(|engine| engine.as_str())
            .unwrap_or("not selected")
    );
    println!(
        "Active output engine: {}",
        status
            .active_output_engine
            .map(|engine| engine.as_str())
            .unwrap_or("not selected")
    );
    println!(
        "Selected devices: input={} output={}",
        status
            .selected_input_device_id
            .as_deref()
            .unwrap_or("system default"),
        status
            .selected_output_device_id
            .as_deref()
            .unwrap_or("system default")
    );
    println!(
        "Policy: tts_approval={} stt_approval={} remote_input={} remote_output={} persist_transcripts={}",
        status.policy.require_approval_for_tts,
        status.policy.require_approval_for_stt,
        status.policy.allow_remote_audio_input,
        status.policy.allow_remote_audio_output,
        status.policy.persist_transcripts
    );

    if !status.warnings.is_empty() {
        println!("Warnings:");
        for warning in &status.warnings {
            println!("- {}", warning);
        }
    }

    print_devices(status);

    println!("Engines:");
    for engine in &status.engines {
        println!(
            "- {} [{}] installed={} enabled={} readiness={} asset_status={}{}{}",
            engine.name,
            engine.kind.as_str(),
            engine.installed,
            engine.enabled,
            engine.readiness.as_str(),
            engine.asset_status.as_str(),
            if engine.active_input {
                " active-input"
            } else {
                ""
            },
            if engine.active_output {
                " active-output"
            } else {
                ""
            }
        );
        println!(
            "  supports_input={} supports_output={} approval_input={} approval_output={}",
            engine.supports_input,
            engine.supports_output,
            engine.approval_required_for_input,
            engine.approval_required_for_output
        );
        if let Some(model) = &engine.model {
            println!("  model={}", model);
        }
        if let Some(voice) = &engine.voice {
            println!("  voice={}", voice);
        }
        if let Some(runtime_path) = &engine.runtime_path {
            println!("  runtime_path={}", runtime_path);
        }
        if let Some(asset_dir) = &engine.asset_dir {
            println!("  asset_dir={}", asset_dir);
        }
        if let Some(notes) = &engine.notes {
            println!("  notes={}", notes);
        }
        for warning in &engine.warnings {
            println!("  warning: {}", warning);
        }
    }
}

fn print_devices(status: &sdk::VoiceSurfaceStatus) {
    if status.devices.is_empty() {
        println!("Devices: none visible");
        return;
    }

    println!("Devices:");
    for device in &status.devices {
        println!(
            "- {} [{}] kind={} default={} available={}",
            device.name,
            device.id,
            device.kind.as_str(),
            device.default,
            device.available
        );
    }
}
