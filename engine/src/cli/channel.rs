use anyhow::Result;

use std::sync::Arc;

use crate::channels::manager::{ChannelManager, PluginChannelDeliverInput, TelegramSetupInput};
use crate::cli::bootstrap::init_daemon;
use crate::config::Config;

use super::commands::{ChannelAction, ChannelPluginAction, ChannelTelegramAction};

pub async fn handle_channels(action: ChannelAction, config: &Config) -> Result<()> {
    let manager = ChannelManager::new(config.clone());
    match action {
        ChannelAction::List => {
            for channel in manager.list().await? {
                println!(
                    "{}\t{}\t{}",
                    channel.name,
                    if channel.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    if channel.configured {
                        "configured"
                    } else {
                        "needs setup"
                    }
                );
            }
            Ok(())
        }
        ChannelAction::Plugin { action } => handle_plugin(action, manager, config).await,
        ChannelAction::Telegram { action } => handle_telegram(action, manager).await,
    }
}

async fn handle_plugin(
    action: ChannelPluginAction,
    manager: ChannelManager,
    _config: &Config,
) -> Result<()> {
    match action {
        ChannelPluginAction::Status => {
            println!(
                "{}",
                serde_json::to_string_pretty(&manager.plugin_statuses().await?)?
            );
        }
        ChannelPluginAction::Deliver {
            name,
            input,
            session_id,
            workspace,
            team_id,
        } => {
            let (_, _, gateway) = init_daemon().await?;
            let result = manager
                .deliver_plugin(
                    &name,
                    PluginChannelDeliverInput {
                        input,
                        session_id,
                        workspace,
                        team_id,
                    },
                    Arc::clone(&gateway),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}

async fn handle_telegram(action: ChannelTelegramAction, manager: ChannelManager) -> Result<()> {
    match action {
        ChannelTelegramAction::Status => {
            println!(
                "{}",
                serde_json::to_string_pretty(&manager.telegram_status().await?)?
            );
        }
        ChannelTelegramAction::Setup {
            token,
            allow_user,
            confirmation_chat,
            api_base_url,
            agent,
        } => {
            let status = manager
                .telegram_setup(TelegramSetupInput {
                    token,
                    allowed_ids: allow_user,
                    confirmation_chat_id: confirmation_chat,
                    api_base_url,
                    default_agent_id: agent,
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        ChannelTelegramAction::Enable => {
            let status = manager.telegram_set_enabled(true).await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        ChannelTelegramAction::Disable => {
            let status = manager.telegram_set_enabled(false).await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        ChannelTelegramAction::Test => {
            let result = manager.telegram_test().await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        ChannelTelegramAction::Doctor => {
            let status = manager.telegram_status().await?;
            for line in status.doctor {
                println!("- {}", line);
            }
        }
    }
    Ok(())
}
