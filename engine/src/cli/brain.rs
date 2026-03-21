mod check;
mod dispatch_family;
mod guide;
mod install;
mod list;
mod remove;
mod server;
mod status;
mod verify;

use anyhow::{bail, Result};

use super::commands::{BrainAction, BrainFamilyArg};

pub async fn execute(action: BrainAction) -> Result<()> {
    match action {
        BrainAction::Check => check::run().await,
        BrainAction::Setup => guide::show(),
        BrainAction::Status { family } => match family.unwrap_or(BrainFamilyArg::Reasoning) {
            BrainFamilyArg::Dispatch => dispatch_family::status(),
            _ => status::run().await,
        },
        BrainAction::Install { target, source } => {
            let (family, model) = parse_target(&target)?;
            match family {
                BrainFamilyArg::Dispatch => {
                    dispatch_family::install(&model, source.as_deref()).await
                }
                _ => install::run(&model).await,
            }
        }
        BrainAction::List { family } => match family.unwrap_or(BrainFamilyArg::Reasoning) {
            BrainFamilyArg::Dispatch => dispatch_family::list(),
            _ => list::run(),
        },
        BrainAction::Use { family, model } => match family {
            BrainFamilyArg::Dispatch => dispatch_family::use_model(&model),
            BrainFamilyArg::Reasoning => {
                println!(
                    "Reasoning brains are selected when you run `rove brain start --model {}`.",
                    model
                );
                Ok(())
            }
            _ => bail!(
                "The '{}' brain family does not support selection yet.",
                family.as_str()
            ),
        },
        BrainAction::Remove { target } => {
            let (family, model) = parse_target(&target)?;
            match family {
                BrainFamilyArg::Dispatch => dispatch_family::remove(&model),
                _ => remove::run(&model),
            }
        }
        BrainAction::Start {
            family,
            model,
            port,
        } => match family.unwrap_or(BrainFamilyArg::Reasoning) {
            BrainFamilyArg::Reasoning => server::start(model.as_deref(), port),
            other => bail!(
                "The '{}' brain family does not use `rove brain start`.",
                other.as_str()
            ),
        },
        BrainAction::Stop => server::stop(),
        BrainAction::Verify { family } => match family.unwrap_or(BrainFamilyArg::Reasoning) {
            BrainFamilyArg::Reasoning => verify::run().await,
            other => bail!(
                "The '{}' brain family does not expose `rove brain verify` yet.",
                other.as_str()
            ),
        },
    }
}

pub(crate) fn stop_local_server() -> Result<()> {
    server::stop_background()
}

fn parse_target(target: &[String]) -> Result<(BrainFamilyArg, String)> {
    match target {
        [] => bail!("Brain command needs a target. Example: `rove brain install dispatch bert-tiny`."),
        [model] => Ok((BrainFamilyArg::Reasoning, model.clone())),
        [family, model, ..] => {
            if let Some(family) = parse_family_name(family) {
                Ok((family, model.clone()))
            } else {
                Ok((BrainFamilyArg::Reasoning, target.join(" ")))
            }
        }
    }
}

fn parse_family_name(value: &str) -> Option<BrainFamilyArg> {
    match value {
        "dispatch" => Some(BrainFamilyArg::Dispatch),
        "reasoning" => Some(BrainFamilyArg::Reasoning),
        "embedding" => Some(BrainFamilyArg::Embedding),
        "rerank" => Some(BrainFamilyArg::Rerank),
        "vision" => Some(BrainFamilyArg::Vision),
        _ => None,
    }
}
