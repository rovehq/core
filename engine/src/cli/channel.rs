use anyhow::Result;

use crate::channels::manager::{ChannelManager, TelegramSetupInput};
use crate::config::Config;

use super::commands::{ChannelAction, ChannelTelegramAction};

pub async fn handle_channels(action: ChannelAction, config: &Config) -> Result<()> {
    let manager = ChannelManager::new(config.clone());
    match action {
        ChannelAction::List => {
            for channel in manager.list().await? {
                println!(
                    "{}\t{}\t{}",
                    channel.name,
                    if channel.enabled { "enabled" } else { "disabled" },
                    if channel.configured { "configured" } else { "needs setup" }
                );
            }
            Ok(())
        }
        ChannelAction::Telegram { action } => handle_telegram(action, manager).await,
    }
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
