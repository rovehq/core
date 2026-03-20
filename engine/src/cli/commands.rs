use std::path::PathBuf;

use clap::{Parser, Subcommand};

use super::output::TaskView;

/// Rove command-line interface.
#[derive(Parser, Debug)]
#[command(
    name = "rove",
    version,
    about = "Rove - Autonomous AI Agent Engine",
    long_about = "Rove is a local-first, plugin-driven AI agent engine.\n\nRun `rove` with no arguments to enter interactive mode.",
    after_help = "Examples:\n  rove                          Start interactive mode\n  rove start                    Start daemon in background\n  rove task \"do something\"      Execute a task immediately\n  rove history                  Show recent tasks\n  rove replay <task-id>         Show task steps\n  rove model list               List configured LLM providers\n  rove schedule add daily-brief --every-minutes 1440 \"prepare my morning brief\""
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Optional path to a configuration file.
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Enable verbose logging.
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the Rove daemon in background.
    Start {
        /// Port to bind the server to.
        #[arg(short, long, default_value_t = 3727)]
        port: u16,
    },

    /// Stop the running Rove daemon.
    Stop,

    /// Execute a task immediately.
    #[command(alias = "run")]
    Task {
        /// The task description.
        prompt: Vec<String>,

        /// Auto-approve destructive actions.
        #[arg(short = 'y', long)]
        yes: bool,

        /// Stream task progress while it runs.
        #[arg(long)]
        stream: bool,

        /// Presentation mode for task execution.
        #[arg(long, value_enum, default_value_t = TaskView::Clean)]
        view: TaskView,
    },

    /// Show recent task history.
    History {
        /// Maximum number of tasks to show.
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },

    /// Replay a task and show every recorded step.
    Replay {
        /// Task UUID to replay.
        task_id: String,
    },

    /// Show daemon and local environment status.
    Status,

    /// Unlock secrets from the keychain for this process.
    Unlock,

    /// Plugin management.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },

    /// Steering management.
    Steer {
        #[command(subcommand)]
        action: SteeringAction,

        /// Optional steering directory override.
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },

    /// LLM model/provider management.
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },

    /// Recurring background task management.
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
    },

    /// Configuration management.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Secret management.
    Secrets {
        #[command(subcommand)]
        action: SecretsAction,
    },

    /// MCP server management.
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },

    /// Local brain management.
    Brain {
        #[command(subcommand)]
        action: BrainAction,
    },

    /// Run the daemon in foreground.
    #[command(hide = true)]
    Daemon {
        /// Port to bind the HTTP server to.
        #[arg(short, long, default_value_t = 3727)]
        port: u16,
    },

    /// Run system diagnostics.
    Doctor,

    /// Generate or verify signing keys.
    Keys,

    /// Update Rove to the latest version.
    Update {
        /// Check whether an update is available without applying it.
        #[arg(long)]
        check: bool,
    },

    /// Interactively configure Rove.
    Setup,
}

#[derive(Subcommand, Debug)]
pub enum PluginAction {
    /// Install a plugin.
    Install { source: String },
    /// Upgrade or replace an installed plugin from a package directory.
    Upgrade { source: String },
    /// List installed plugins.
    List,
    /// Show one installed plugin.
    Inspect { name: String },
    /// Enable an installed plugin.
    Enable { name: String },
    /// Disable an installed plugin.
    Disable { name: String },
    /// Remove a plugin.
    Remove { name: String },
}

#[derive(Subcommand, Debug)]
pub enum SteeringAction {
    /// List all loaded steering files.
    List,
    /// Show currently active steering files.
    #[command(alias = "active")]
    Status,
    /// Activate a steering file by exact name.
    On { name: String },
    /// Deactivate a steering file by exact name.
    Off { name: String },
    /// Restore built-in steering defaults if missing.
    Default,
}

#[derive(Subcommand, Debug)]
pub enum ModelAction {
    /// Download a model.
    Pull { name: String },
    /// List configured providers.
    List,
    /// Interactively add or configure an LLM provider.
    Setup,
}

#[derive(Subcommand, Debug)]
pub enum ScheduleAction {
    /// Add a recurring background task.
    Add {
        /// Unique schedule name.
        name: String,

        /// Repeat interval in minutes.
        #[arg(long, value_name = "MINUTES")]
        every_minutes: u64,

        /// Queue the first run immediately.
        #[arg(long)]
        start_now: bool,

        /// Task prompt to enqueue.
        prompt: Vec<String>,
    },
    /// List recurring background tasks.
    List,
    /// Show one recurring background task.
    Show { name: String },
    /// Pause a recurring background task.
    Pause { name: String },
    /// Resume a recurring background task.
    Resume { name: String },
    /// Queue the next run immediately.
    #[command(name = "run-now")]
    RunNow { name: String },
    /// Remove a recurring background task.
    Remove { name: String },
}

#[derive(Subcommand, Debug)]
pub enum BrainAction {
    /// Check if llama-server is available.
    Check,
    /// Show installation instructions for llama.cpp.
    Setup,
    /// Show local brain status.
    Status,
    /// Install a model.
    Install { model: String },
    /// List installed models.
    List,
    /// Remove a model.
    Remove { model: String },
    /// Start llama-server with an installed model.
    Start {
        /// Model name to start.
        #[arg(short, long)]
        model: Option<String>,

        /// Port for llama-server.
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
    /// Stop the running llama-server.
    Stop,
    /// Verify llama-server is responding.
    Verify,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show the current configuration with sensitive fields masked.
    Show,
}

#[derive(Subcommand, Debug)]
pub enum SecretsAction {
    /// Store a secret in the configured secret backend.
    Set { name: String },
    /// List known secret slots and whether they are configured.
    List,
    /// Remove a stored secret.
    Remove { name: String },
}

#[derive(Subcommand, Debug)]
pub enum McpAction {
    /// List configured MCP servers.
    List,
    /// Show one configured MCP server.
    Show { name: String },
    /// List built-in and installed MCP templates.
    Templates,
    /// Add a configured MCP server.
    Add {
        /// Stable server name.
        name: String,

        /// Template name to apply.
        #[arg(long, default_value = "custom")]
        template: String,

        /// Executable command for the MCP server.
        #[arg(long)]
        command: Option<String>,

        /// Repeated argument passed to the MCP server command.
        #[arg(long = "arg")]
        args: Vec<String>,

        /// Optional human description.
        #[arg(long)]
        description: Option<String>,

        /// Allow outbound network access for this server.
        #[arg(long)]
        allow_network: bool,

        /// Allow temporary file access.
        #[arg(long)]
        allow_tmp: bool,

        /// Allow reading the current workspace.
        #[arg(long)]
        workspace_read: bool,

        /// Allow writing the current workspace.
        #[arg(long)]
        workspace_write: bool,

        /// Additional allowed read path.
        #[arg(long = "read", value_name = "PATH")]
        read_paths: Vec<PathBuf>,

        /// Additional allowed write path.
        #[arg(long = "write", value_name = "PATH")]
        write_paths: Vec<PathBuf>,

        /// Add the server in disabled state.
        #[arg(long)]
        disabled: bool,
    },
    /// Enable a configured MCP server.
    Enable { name: String },
    /// Disable a configured MCP server.
    Disable { name: String },
    /// Remove a configured MCP server.
    Remove { name: String },
    /// Verify that a configured MCP server starts and responds to tools/list.
    Test { name: String },
    /// List tools exposed by a configured MCP server.
    Tools { name: String },
}
