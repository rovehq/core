use anyhow::{bail, Result};
use zeroize::Zeroize;

use crate::cli::commands::AuthAction;
use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::security::{
    can_reset_with_device_secret, describe_protection_state, password_protection_state,
    reset_password_for_config, verify_recovery_code, PasswordProtectionState,
};
use crate::storage::Database;

pub async fn handle_auth(action: AuthAction) -> Result<()> {
    match action {
        AuthAction::Status => auth_status().await,
        AuthAction::ResetPassword { recovery_code } => reset_password(recovery_code).await,
    }
}

async fn auth_status() -> Result<()> {
    let config = Config::load_or_create()?;
    let config_path = Config::config_path()?;
    let state = password_protection_state(&config)?;

    println!("Daemon auth: {}", describe_protection_state(state));
    println!(
        "Device reset available: {}",
        if can_reset_with_device_secret(&config_path, &config.webui)? {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "Recovery code configured: {}",
        if config.webui.recovery_code_hash.is_some() {
            "yes"
        } else {
            "no"
        }
    );
    Ok(())
}

async fn reset_password(recovery_code: Option<String>) -> Result<()> {
    let mut config = Config::load_or_create()?;
    let config_path = Config::config_path()?;
    let state = password_protection_state(&config)?;
    if matches!(state, PasswordProtectionState::Uninitialized) {
        bail!("Daemon password has not been configured yet");
    }

    let can_use_device_secret = can_reset_with_device_secret(&config_path, &config.webui)?;
    let recovery_allowed = if can_use_device_secret {
        true
    } else {
        let Some(code) = recovery_code.as_deref() else {
            bail!(
                "Device reset is unavailable. Re-run with `rove auth reset-password --recovery-code <code>`"
            );
        };
        verify_recovery_code(&config.webui, code)?
    };

    if !can_use_device_secret && !recovery_allowed {
        bail!("Recovery code was invalid");
    }

    let mut password = rpassword::read_password_from_tty(Some("New daemon password: "))?;
    if password.trim().len() < 8 {
        password.zeroize();
        bail!("Password must be at least 8 characters");
    }
    let mut confirm = rpassword::read_password_from_tty(Some("Confirm daemon password: "))?;
    if password != confirm {
        password.zeroize();
        confirm.zeroize();
        bail!("Passwords did not match");
    }
    confirm.zeroize();

    let auth_artifacts =
        reset_password_for_config(&config_path, &mut config.webui, password.as_str())?;
    password.zeroize();
    config.save()?;

    let db = Database::new(&database_path(&config)).await?;
    db.auth().revoke_all_sessions().await?;

    println!(
        "Daemon password reset complete: {}",
        describe_protection_state(auth_artifacts.protection_state)
    );
    println!("New recovery code: {}", auth_artifacts.recovery_code);
    println!("All existing sessions were revoked.");
    Ok(())
}
