mod check;
mod guide;
mod install;
mod list;
mod remove;
mod server;
mod status;
mod verify;

use anyhow::Result;

use super::commands::BrainAction;

pub async fn execute(action: BrainAction) -> Result<()> {
    match action {
        BrainAction::Check => check::run().await,
        BrainAction::Setup => guide::show(),
        BrainAction::Status => status::run().await,
        BrainAction::Install { model } => install::run(&model).await,
        BrainAction::List => list::run(),
        BrainAction::Remove { model } => remove::run(&model),
        BrainAction::Start { model, port } => server::start(model.as_deref(), port),
        BrainAction::Stop => server::stop(),
        BrainAction::Verify => verify::run().await,
    }
}

pub(crate) fn stop_local_server() -> Result<()> {
    server::stop_background()
}
