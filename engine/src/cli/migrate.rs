use anyhow::Result;

use crate::cli::commands::{MigrateAction, MigrationSourceArg};
use crate::system::migrate::{self, MigrationSource};
use crate::system::specs::SpecRepository;

pub fn handle_migrate(action: MigrateAction) -> Result<()> {
    match action {
        MigrateAction::Inspect { source, path } => {
            let report = migrate::inspect(map_source(source), path.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        MigrateAction::Import { source, path } => {
            let repo = SpecRepository::new()?;
            let result = migrate::import(&repo, map_source(source), path.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
    }
}

fn map_source(source: MigrationSourceArg) -> MigrationSource {
    match source {
        MigrationSourceArg::Openclaw => MigrationSource::OpenClaw,
        MigrationSourceArg::Zeroclaw => MigrationSource::ZeroClaw,
        MigrationSourceArg::Moltis => MigrationSource::Moltis,
    }
}
