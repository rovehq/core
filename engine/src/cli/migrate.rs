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
        MigrateAction::Import {
            source,
            path,
            dry_run,
        } => {
            let repo = SpecRepository::new()?;
            let result = migrate::import(&repo, map_source(source), path.as_deref(), dry_run)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        MigrateAction::Status => {
            let repo = SpecRepository::new()?;
            let report = migrate::migrate_status(&repo)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
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
