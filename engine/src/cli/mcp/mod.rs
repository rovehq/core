mod scaffold;
mod servers;
mod templates;

use std::path::PathBuf;

use anyhow::Result;

use crate::cli::McpAction;
use crate::config::Config;

pub async fn handle(action: McpAction, config: &Config) -> Result<()> {
    match action {
        McpAction::List => servers::list_servers(config).await,
        McpAction::Show { name } => servers::show_server(config, &name).await,
        McpAction::Install { source } => servers::install_package(config, &source).await,
        McpAction::Upgrade { source } => servers::upgrade_package(config, &source).await,
        McpAction::Export {
            name,
            dir,
            package_name,
        } => servers::export_server(config, &name, dir, package_name.as_deref()).await,
        McpAction::Scaffold {
            dir,
            name,
            template,
            server_name,
            command,
            args,
            description,
            allow_network,
            allow_tmp,
            read_paths,
            write_paths,
        } => {
            let request = ScaffoldRequest {
                dir,
                name,
                template,
                server_name,
                command,
                args,
                description,
                allow_network,
                allow_tmp,
                read_paths,
                write_paths,
            };
            scaffold::generate_package(config, request)
        }
        McpAction::Templates => templates::list_templates(config),
        McpAction::Add {
            name,
            template,
            command,
            args,
            description,
            allow_network,
            allow_tmp,
            workspace_read,
            workspace_write,
            read_paths,
            write_paths,
            disabled,
        } => {
            let request = AddServerRequest {
                name,
                template,
                command,
                args,
                description,
                allow_network,
                allow_tmp,
                workspace_read,
                workspace_write,
                read_paths,
                write_paths,
                disabled,
            };
            servers::add_server(config, request).await
        }
        McpAction::Enable { name } => servers::set_enabled(config, &name, true).await,
        McpAction::Disable { name } => servers::set_enabled(config, &name, false).await,
        McpAction::Remove { name } => servers::remove_server(config, &name).await,
        McpAction::Test { name } => servers::test_server(config, &name).await,
        McpAction::Tools { name } => servers::list_server_tools(config, &name).await,
    }
}

#[derive(Debug)]
pub(super) struct AddServerRequest {
    pub(super) name: String,
    pub(super) template: String,
    pub(super) command: Option<String>,
    pub(super) args: Vec<String>,
    pub(super) description: Option<String>,
    pub(super) allow_network: bool,
    pub(super) allow_tmp: bool,
    pub(super) workspace_read: bool,
    pub(super) workspace_write: bool,
    pub(super) read_paths: Vec<PathBuf>,
    pub(super) write_paths: Vec<PathBuf>,
    pub(super) disabled: bool,
}

#[derive(Debug)]
pub(super) struct ScaffoldRequest {
    pub(super) dir: PathBuf,
    pub(super) name: String,
    pub(super) template: String,
    pub(super) server_name: Option<String>,
    pub(super) command: Option<String>,
    pub(super) args: Vec<String>,
    pub(super) description: Option<String>,
    pub(super) allow_network: bool,
    pub(super) allow_tmp: bool,
    pub(super) read_paths: Vec<PathBuf>,
    pub(super) write_paths: Vec<PathBuf>,
}
