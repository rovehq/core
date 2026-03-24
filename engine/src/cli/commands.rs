use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

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
        #[arg(short, long, default_value_t = crate::config::metadata::DEFAULT_PORT)]
        port: u16,

        /// Runtime profile for the daemon.
        #[arg(long, value_enum)]
        profile: Option<DaemonProfileArg>,
    },

    /// Stop the running Rove daemon.
    Stop,

    /// Execute a task immediately.
    #[command(alias = "run")]
    Task {
        /// The task description.
        prompt: Vec<String>,

        /// Explicit remote node target.
        #[arg(long)]
        node: Option<String>,

        /// Auto-approve destructive actions.
        #[arg(short = 'y', long)]
        yes: bool,

        /// Stream task progress while it runs.
        #[arg(long)]
        stream: bool,

        /// Run this task in its own top-level run context.
        #[arg(long)]
        parallel: bool,

        /// Explicit workspace isolation mode for parallel write-heavy tasks.
        #[arg(long, value_enum)]
        isolate: Option<TaskIsolationArg>,

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

    /// Manage reusable agent specs.
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },

    /// Manage reusable workflow specs.
    Workflow {
        #[command(subcommand)]
        action: WorkflowAction,
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
    #[command(hide = true)]
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },

    /// Legacy steering management alias.
    #[command(hide = true)]
    Steer {
        #[command(subcommand)]
        action: SteeringAction,

        /// Optional legacy steering directory override.
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

    /// Approval mode and allowlist rule management.
    Approvals {
        #[command(subcommand)]
        action: ApprovalsAction,
    },

    /// MCP server management.
    #[command(hide = true)]
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },

    /// Policy management.
    #[command(alias = "steering")]
    Policy {
        #[command(subcommand)]
        action: PolicyAction,

        /// Optional policy directory override.
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },

    /// Unified extension management across skills, systems, connectors, and channels.
    Extension {
        #[command(subcommand)]
        action: ExtensionFacadeAction,
    },

    /// Skill extension management.
    Skill {
        #[command(subcommand)]
        action: ExtensionAction,
    },

    /// System extension management.
    System {
        #[command(subcommand)]
        action: ExtensionAction,
    },

    /// Connector management.
    Connector {
        #[command(subcommand)]
        action: McpAction,
    },

    /// Runtime channel management.
    Channel {
        #[command(subcommand)]
        action: ChannelAction,
    },

    /// Optional service management.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },

    /// Remote node and mesh management.
    Remote {
        #[command(subcommand)]
        action: RemoteAction,
    },

    /// Friendly install shortcut for common optional capabilities.
    Add {
        #[arg(value_enum)]
        target: AddTarget,
    },

    /// Friendly enable shortcut for common services.
    Activate {
        #[arg(value_enum)]
        target: ActivateTarget,
    },

    /// Friendly disable shortcut for common services.
    Deactivate {
        #[arg(value_enum)]
        target: ActivateTarget,
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
        #[arg(short, long, default_value_t = crate::config::metadata::DEFAULT_PORT)]
        port: u16,

        /// Runtime profile for the daemon.
        #[arg(long, value_enum)]
        profile: Option<DaemonProfileArg>,
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
    /// Create a new plugin authoring scaffold.
    New {
        /// Directory name for the new plugin package.
        name: String,

        /// Plugin surface to scaffold.
        #[arg(long = "type", value_enum, default_value_t = PluginScaffoldType::Skill)]
        plugin_type: PluginScaffoldType,
    },
    /// Build and run a local plugin package against a mock runtime.
    Test {
        /// Plugin package directory. Defaults to the current directory.
        source: Option<String>,

        /// Specific exported tool to call.
        #[arg(long)]
        tool: Option<String>,

        /// Primary task input for the plugin.
        #[arg(long)]
        input: Option<String>,

        /// File paths to include in the plugin input.
        #[arg(long = "file", value_name = "FILE")]
        files: Vec<PathBuf>,

        /// Additional plugin input fields in key=value form.
        #[arg(long = "arg", value_name = "KEY=VALUE")]
        args: Vec<String>,

        /// Skip cargo test/build before executing the plugin.
        #[arg(long)]
        no_build: bool,
    },
    /// Create a normalized distribution bundle directory.
    Pack {
        /// Plugin package directory. Defaults to the current directory.
        source: Option<String>,

        /// Optional output directory for the generated bundle.
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,

        /// Skip cargo test/build before packing.
        #[arg(long)]
        no_build: bool,
    },
    /// Publish a bundled plugin into a registry directory structure.
    Publish {
        /// Plugin package directory. Defaults to the current directory.
        source: Option<String>,

        /// Registry directory that will receive id/version bundles.
        #[arg(long = "registry-dir", value_name = "DIR")]
        registry_dir: PathBuf,

        /// Skip cargo test/build before publishing.
        #[arg(long)]
        no_build: bool,
    },
    /// Install a plugin from a local package directory or a registry.
    Install {
        /// Package directory, or plugin id when --registry is set.
        source: String,

        /// Static registry directory or base URL.
        #[arg(long, value_name = "REGISTRY")]
        registry: Option<String>,

        /// Specific version to install from the registry.
        #[arg(long)]
        version: Option<String>,
    },
    /// Upgrade or replace an installed plugin from a package directory or registry.
    Upgrade {
        /// Package directory, or plugin id when --registry is set.
        source: String,

        /// Static registry directory or base URL.
        #[arg(long, value_name = "REGISTRY")]
        registry: Option<String>,

        /// Specific version to upgrade to from the registry.
        #[arg(long)]
        version: Option<String>,
    },
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
pub enum ExtensionAction {
    /// Create a new extension authoring scaffold.
    New {
        /// Directory name for the new extension package.
        name: String,
    },
    /// Build and run a local extension package against a mock runtime.
    Test {
        /// Extension package directory. Defaults to the current directory.
        source: Option<String>,

        /// Specific exported tool to call.
        #[arg(long)]
        tool: Option<String>,

        /// Primary task input for the extension.
        #[arg(long)]
        input: Option<String>,

        /// File paths to include in the extension input.
        #[arg(long = "file", value_name = "FILE")]
        files: Vec<PathBuf>,

        /// Additional extension input fields in key=value form.
        #[arg(long = "arg", value_name = "KEY=VALUE")]
        args: Vec<String>,

        /// Skip cargo test/build before executing the extension.
        #[arg(long)]
        no_build: bool,
    },
    /// Create a normalized distribution bundle directory.
    Pack {
        /// Extension package directory. Defaults to the current directory.
        source: Option<String>,

        /// Optional output directory for the generated bundle.
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,

        /// Skip cargo test/build before packing.
        #[arg(long)]
        no_build: bool,
    },
    /// Publish a bundled extension into a registry directory structure.
    Publish {
        /// Extension package directory. Defaults to the current directory.
        source: Option<String>,

        /// Registry directory that will receive id/version bundles.
        #[arg(long = "registry-dir", value_name = "DIR")]
        registry_dir: PathBuf,

        /// Skip cargo test/build before publishing.
        #[arg(long)]
        no_build: bool,
    },
    /// Install an extension from a local package directory or a registry.
    Install {
        /// Package directory, or extension id when --registry is set.
        source: String,

        /// Static registry directory or base URL.
        #[arg(long, value_name = "REGISTRY")]
        registry: Option<String>,

        /// Specific version to install from the registry.
        #[arg(long)]
        version: Option<String>,
    },
    /// Upgrade or replace an installed extension from a package directory or registry.
    Upgrade {
        /// Package directory, or extension id when --registry is set.
        source: String,

        /// Static registry directory or base URL.
        #[arg(long, value_name = "REGISTRY")]
        registry: Option<String>,

        /// Specific version to upgrade to from the registry.
        #[arg(long)]
        version: Option<String>,
    },
    /// List installed extensions of this kind.
    List,
    /// Show one installed extension.
    Inspect { name: String },
    /// Enable an installed extension.
    Enable { name: String },
    /// Disable an installed extension.
    Disable { name: String },
    /// Remove an extension.
    Remove { name: String },
}

#[derive(Subcommand, Debug)]
pub enum ExtensionFacadeAction {
    /// Create a new extension authoring scaffold.
    New {
        /// Extension kind to scaffold.
        #[arg(value_enum)]
        kind: ExtensionKindArg,

        /// Directory name for the new extension package.
        name: String,
    },
    /// Build and run a local extension package against a mock runtime.
    Test {
        /// Extension kind to test.
        #[arg(value_enum)]
        kind: ExtensionKindArg,

        /// Extension package directory. Defaults to the current directory.
        source: Option<String>,

        /// Specific exported tool to call.
        #[arg(long)]
        tool: Option<String>,

        /// Primary task input for the extension.
        #[arg(long)]
        input: Option<String>,

        /// File paths to include in the extension input.
        #[arg(long = "file", value_name = "FILE")]
        files: Vec<PathBuf>,

        /// Additional extension input fields in key=value form.
        #[arg(long = "arg", value_name = "KEY=VALUE")]
        args: Vec<String>,

        /// Skip cargo test/build before executing the extension.
        #[arg(long)]
        no_build: bool,
    },
    /// Create a normalized distribution bundle directory.
    Pack {
        /// Extension kind to pack.
        #[arg(value_enum)]
        kind: ExtensionKindArg,

        /// Extension package directory. Defaults to the current directory.
        source: Option<String>,

        /// Optional output directory for the generated bundle.
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,

        /// Skip cargo test/build before packing.
        #[arg(long)]
        no_build: bool,
    },
    /// Publish a bundled extension into a registry directory structure.
    Publish {
        /// Extension kind to publish.
        #[arg(value_enum)]
        kind: ExtensionKindArg,

        /// Extension package directory. Defaults to the current directory.
        source: Option<String>,

        /// Registry directory that will receive id/version bundles.
        #[arg(long = "registry-dir", value_name = "DIR")]
        registry_dir: PathBuf,

        /// Skip cargo test/build before publishing.
        #[arg(long)]
        no_build: bool,
    },
    /// Install an extension from a local package directory or a registry.
    Install {
        /// Extension kind to install. Omit to resolve from the public catalog.
        #[arg(value_enum)]
        kind: Option<ExtensionKindArg>,

        /// Extension id from the public catalog, or a package directory / explicit source in developer mode.
        source: String,

        /// Static registry directory or base URL.
        #[arg(long, value_name = "REGISTRY")]
        registry: Option<String>,

        /// Specific version to install from the registry.
        #[arg(long)]
        version: Option<String>,
    },
    /// Upgrade or replace an installed extension from a local package directory or a registry.
    Upgrade {
        /// Extension kind to upgrade. Omit to resolve from the public catalog.
        #[arg(value_enum)]
        kind: Option<ExtensionKindArg>,

        /// Package directory, or extension id when --registry is set.
        source: String,

        /// Static registry directory or base URL.
        #[arg(long, value_name = "REGISTRY")]
        registry: Option<String>,

        /// Specific version to upgrade to from the registry.
        #[arg(long)]
        version: Option<String>,
    },
    /// Search the public extension catalog.
    Search {
        /// Optional search term.
        query: Option<String>,
    },
    /// Show a public catalog extension.
    Show {
        /// Extension id from the public catalog.
        id: String,
    },
    /// Show installed extensions with newer catalog versions available.
    Updates,
    /// List installed extensions, optionally filtered by kind.
    List {
        /// Limit the list to one extension kind.
        #[arg(value_enum)]
        kind: Option<ExtensionKindArg>,
    },
    /// Show one installed extension.
    Inspect {
        /// Extension kind.
        #[arg(value_enum)]
        kind: ExtensionKindArg,

        /// Extension name or id.
        name: String,
    },
    /// Enable an installed extension.
    Enable {
        /// Extension kind.
        #[arg(value_enum)]
        kind: ExtensionKindArg,

        /// Extension name or id.
        name: String,
    },
    /// Disable an installed extension.
    Disable {
        /// Extension kind.
        #[arg(value_enum)]
        kind: ExtensionKindArg,

        /// Extension name or id.
        name: String,
    },
    /// Remove an extension.
    Remove {
        /// Extension kind.
        #[arg(value_enum)]
        kind: ExtensionKindArg,

        /// Extension name or id.
        name: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PluginScaffoldType {
    Skill,
    System,
    Channel,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ExtensionKindArg {
    Skill,
    System,
    Connector,
    Channel,
}

impl ExtensionKindArg {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::System => "system",
            Self::Connector => "connector",
            Self::Channel => "channel",
        }
    }
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
pub enum PolicyAction {
    /// List all loaded policy files.
    List,
    /// Show the current active policy stack.
    #[command(alias = "active")]
    Status,
    /// Show one policy file by exact name.
    Show { name: String },
    /// Enable a policy by exact name.
    Enable { name: String },
    /// Disable a policy by exact name.
    Disable { name: String },
    /// Restore built-in policy defaults if missing.
    Default,
    /// Explain which policies match a task.
    Explain {
        /// Task text to evaluate against active policies.
        task: Vec<String>,
    },
    /// Test policy activation and merged directives for a task.
    Test {
        /// Task text to evaluate against active policies.
        task: Vec<String>,
    },
    /// Add a new user or workspace policy file.
    Add {
        /// Policy name or file stem.
        name: String,

        /// Scope for the new policy.
        #[arg(long, value_enum, default_value_t = PolicyScopeArg::Workspace)]
        scope: PolicyScopeArg,
    },
    /// Remove a policy file.
    Remove { name: String },
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
    Status {
        /// Optional brain family to inspect.
        #[arg(value_enum)]
        family: Option<BrainFamilyArg>,
    },
    /// Install a brain model or artifact family entry.
    Install {
        /// `dispatch bert-tiny` or a compatibility reasoning model like `qwen2.5-coder-0.5b`.
        target: Vec<String>,

        /// Optional source directory for family-specific artifacts.
        #[arg(long, value_name = "DIR")]
        source: Option<PathBuf>,
    },
    /// List installed brains, optionally scoped to one family.
    List {
        /// Optional brain family to list.
        #[arg(value_enum)]
        family: Option<BrainFamilyArg>,
    },
    /// Select the active model for a brain family.
    Use {
        #[arg(value_enum)]
        family: BrainFamilyArg,
        model: String,
    },
    /// Remove a brain model or family entry.
    Remove { target: Vec<String> },
    /// Start llama-server with an installed model.
    Start {
        /// Optional brain family. Reasoning is the default.
        #[arg(long, value_enum)]
        family: Option<BrainFamilyArg>,

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
    Verify {
        /// Optional brain family. Reasoning is the default.
        #[arg(long, value_enum)]
        family: Option<BrainFamilyArg>,
    },
}

#[derive(Subcommand, Debug)]
pub enum AgentAction {
    /// List saved agents.
    List,
    /// Show one saved agent.
    Show { id: String },
    /// Create a basic saved agent.
    Create {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        purpose: Option<String>,
        #[arg(long)]
        instructions: Option<String>,
        #[arg(long = "tool")]
        tool: Vec<String>,
        #[arg(long)]
        disabled: bool,
    },
    /// Enable an agent.
    Enable { id: String },
    /// Disable an agent.
    Disable { id: String },
    /// Run a saved agent once.
    Run { id: String, prompt: Vec<String> },
    /// Export an agent spec to a TOML file.
    Export { id: String, path: PathBuf },
    /// Import an agent spec from a TOML file.
    Import { path: PathBuf },
    /// Show recent agent runs.
    Runs {
        #[arg(short, long, default_value_t = 20)]
        limit: i64,
    },
    /// Preview or create generated agent specs.
    Factory {
        #[command(subcommand)]
        action: AgentFactoryAction,
    },
    /// Create a reusable agent from an existing task.
    FromTask {
        task_id: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum WorkflowAction {
    /// List saved workflows.
    List,
    /// Show one saved workflow.
    Show { id: String },
    /// Create a basic workflow.
    Create {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long = "step")]
        step: Vec<String>,
        #[arg(long = "agent")]
        agent: Vec<String>,
        #[arg(long)]
        disabled: bool,
    },
    /// Enable a workflow.
    Enable { id: String },
    /// Disable a workflow.
    Disable { id: String },
    /// Run a saved workflow once.
    Run { id: String, input: Vec<String> },
    /// Export a workflow spec to a TOML file.
    Export { id: String, path: PathBuf },
    /// Import a workflow spec from a TOML file.
    Import { path: PathBuf },
    /// Show recent workflow runs.
    Runs {
        #[arg(short, long, default_value_t = 20)]
        limit: i64,
    },
    /// Preview or create generated workflow specs.
    Factory {
        #[command(subcommand)]
        action: WorkflowFactoryAction,
    },
    /// Create a reusable workflow from an existing task.
    FromTask {
        task_id: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum AgentFactoryAction {
    /// List built-in agent templates.
    Templates,
    /// Preview a generated agent spec without saving it.
    Preview {
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        requirement: Vec<String>,
    },
    /// Create and save a generated disabled agent spec.
    Create {
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        requirement: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum WorkflowFactoryAction {
    /// List built-in workflow templates.
    Templates,
    /// Preview a generated workflow spec without saving it.
    Preview {
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        requirement: Vec<String>,
    },
    /// Create and save a generated disabled workflow spec.
    Create {
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        requirement: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ChannelAction {
    /// List runtime channels and setup state.
    List,
    /// Manage Telegram as a first-class channel pack.
    Telegram {
        #[command(subcommand)]
        action: ChannelTelegramAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum ChannelTelegramAction {
    /// Show Telegram status and doctor messages.
    Status,
    /// Configure Telegram transport and bind the default handler agent.
    Setup {
        #[arg(long)]
        token: Option<String>,
        #[arg(long = "allow-user")]
        allow_user: Vec<i64>,
        #[arg(long = "confirmation-chat")]
        confirmation_chat: Option<i64>,
        #[arg(long = "api-base-url")]
        api_base_url: Option<String>,
        #[arg(long = "agent")]
        agent: Option<String>,
    },
    /// Enable Telegram polling.
    Enable,
    /// Disable Telegram polling.
    Disable,
    /// Verify Telegram API credentials and connectivity.
    Test,
    /// Print doctor guidance for Telegram setup.
    Doctor,
}

#[derive(Subcommand, Debug)]
pub enum ServiceAction {
    /// List supported services and their states.
    List,
    /// Show one service.
    Show { name: ServiceTarget },
    /// Enable a service.
    Enable { name: ServiceTarget },
    /// Disable a service.
    Disable { name: ServiceTarget },
    /// Show login/boot service installation state.
    InstallStatus,
    /// Install Rove as a login or boot service.
    Install {
        /// Install mode.
        #[arg(value_enum)]
        mode: ServiceInstallModeArg,

        /// Runtime profile for the installed service.
        #[arg(long, value_enum)]
        profile: Option<DaemonProfileArg>,

        /// Port for the installed daemon service.
        #[arg(short, long, default_value_t = crate::config::metadata::DEFAULT_PORT)]
        port: u16,
    },
    /// Remove a login or boot service install.
    Uninstall {
        /// Install mode.
        #[arg(value_enum)]
        mode: ServiceInstallModeArg,
    },
}

#[derive(Subcommand, Debug)]
pub enum RemoteAction {
    /// Show remote service status for this node.
    Status,
    /// Manage official remote transports such as ZeroTier.
    Transport {
        #[command(subcommand)]
        action: RemoteTransportAction,
    },
    /// Inspect and promote discoverable remote candidates.
    Discover {
        #[command(subcommand)]
        action: RemoteDiscoverAction,
    },
    /// Manage paired nodes in the remote mesh.
    Node {
        #[command(subcommand)]
        action: RemoteNodeAction,
    },
    /// Show or update this node's execution profile.
    Profile {
        #[command(subcommand)]
        action: RemoteProfileAction,
    },
    /// Send a task to a specific remote node.
    Send {
        /// Remote node name, or `auto` to pick the best trusted node.
        node: String,

        /// Require one or more node tags when selecting `auto`.
        #[arg(long = "tag")]
        tags: Vec<String>,

        /// Require one or more node capabilities when selecting `auto`.
        #[arg(long = "capability")]
        capabilities: Vec<String>,

        /// Allow executor-only nodes to be selected.
        #[arg(long)]
        allow_executor_only: bool,

        /// Prefer executor-only nodes when using `auto`.
        #[arg(long)]
        prefer_executor_only: bool,

        /// Task prompt to forward.
        prompt: Vec<String>,
    },
    /// Compatibility alias for `rove remote node list`.
    #[command(hide = true)]
    Nodes,
    /// Compatibility alias for `rove remote node rename`.
    #[command(hide = true)]
    Rename { name: String },
    /// Compatibility alias for `rove remote node pair`.
    #[command(hide = true)]
    Pair {
        /// Node name, or a daemon URL when --url is omitted.
        target: String,

        /// Explicit daemon base URL when target is a human node name.
        #[arg(long)]
        url: Option<String>,

        /// Bearer token for the target daemon.
        #[arg(long)]
        token: Option<String>,

        /// Mark the node as executor-only.
        #[arg(long)]
        executor_only: bool,

        /// Optional capability tags for this node.
        #[arg(long = "tag")]
        tags: Vec<String>,

        /// Optional advertised capabilities for this node.
        #[arg(long = "capability")]
        capabilities: Vec<String>,
    },
    /// Compatibility alias for `rove remote node unpair`.
    #[command(hide = true)]
    Unpair { name: String },
    /// Compatibility alias for `rove remote node trust`.
    #[command(hide = true)]
    Trust { name: String },
}

#[derive(Subcommand, Debug)]
pub enum RemoteTransportAction {
    /// Install and enable the official ZeroTier transport integration.
    Install {
        #[arg(value_enum)]
        transport: RemoteTransportArg,
    },
    /// Disable the official ZeroTier transport integration.
    Uninstall {
        #[arg(value_enum)]
        transport: RemoteTransportArg,
    },
    /// Show transport status.
    Status {
        #[arg(value_enum)]
        transport: RemoteTransportArg,
    },
    /// Configure transport network settings.
    Setup {
        #[arg(value_enum)]
        transport: RemoteTransportArg,

        #[arg(long = "network")]
        network_id: String,

        #[arg(long)]
        token_key: Option<String>,

        #[arg(long, default_value_t = true)]
        managed_name_sync: bool,
    },
    /// Join the configured transport network.
    Join {
        #[arg(value_enum)]
        transport: RemoteTransportArg,

        #[arg(long = "network")]
        network_id: Option<String>,
    },
    /// Force a transport refresh and discovery sync.
    Refresh {
        #[arg(value_enum)]
        transport: RemoteTransportArg,
    },
}

#[derive(Subcommand, Debug)]
pub enum RemoteDiscoverAction {
    /// List discovered remote candidates.
    List,
    /// Refresh discovery now.
    Refresh,
    /// Promote or confirm trust for a discovery candidate.
    Trust { candidate_id: String },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum RemoteTransportArg {
    ZeroTier,
}

#[derive(Subcommand, Debug)]
pub enum RemoteNodeAction {
    /// List trusted or paired nodes.
    List,
    /// Rename this node.
    Rename { name: String },
    /// Pair with a remote node descriptor or invite.
    Pair {
        /// Node name, or a daemon URL when --url is omitted.
        target: String,

        /// Explicit daemon base URL when target is a human node name.
        #[arg(long)]
        url: Option<String>,

        /// Bearer token for the target daemon.
        #[arg(long)]
        token: Option<String>,

        /// Mark the node as executor-only.
        #[arg(long)]
        executor_only: bool,

        /// Optional capability tags for this node.
        #[arg(long = "tag")]
        tags: Vec<String>,

        /// Optional advertised capabilities for this node.
        #[arg(long = "capability")]
        capabilities: Vec<String>,
    },
    /// Remove a paired node.
    Unpair { name: String },
    /// Mark a paired node as trusted.
    Trust { name: String },
}

#[derive(Subcommand, Debug)]
pub enum RemoteProfileAction {
    /// Show the local node profile.
    Show,
    /// Set this node to executor-only mode.
    ExecutorOnly,
    /// Set this node back to full execution mode.
    Full,
    /// Replace this node's capability tags.
    Tags {
        /// Repeated tag value.
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// Replace this node's advertised capabilities.
    Capabilities {
        /// Repeated capability value.
        #[arg(long = "capability")]
        capabilities: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum BrainFamilyArg {
    Dispatch,
    Reasoning,
    Embedding,
    Rerank,
    Vision,
}

impl BrainFamilyArg {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::Reasoning => "reasoning",
            Self::Embedding => "embedding",
            Self::Rerank => "rerank",
            Self::Vision => "vision",
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
pub enum PolicyScopeArg {
    User,
    Workspace,
    Project,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum ServiceTarget {
    Logging,
    Webui,
    Remote,
    #[value(name = "connector-engine")]
    ConnectorEngine,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum ServiceInstallModeArg {
    Login,
    Boot,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum TaskIsolationArg {
    Worktree,
    Snapshot,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum AddTarget {
    Mcp,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum ActivateTarget {
    Logging,
    Webui,
    Remote,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DaemonProfileArg {
    Desktop,
    Headless,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show the current configuration with sensitive fields masked.
    Show,
    /// Reload and validate config from disk.
    Reload,
}

#[derive(Subcommand, Debug)]
pub enum SecretsAction {
    /// Store a secret in the configured secret backend.
    Set { name: String },
    /// List known secret slots and whether they are configured.
    List,
    /// Remove a stored secret.
    Remove { name: String },
    /// Show or change the configured secret backend.
    Backend {
        #[command(subcommand)]
        action: SecretBackendAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum SecretBackendAction {
    /// Show the configured backend.
    Show,
    /// Set the configured backend.
    Set { backend: SecretBackendArg },
}

#[derive(Subcommand, Debug)]
pub enum ApprovalsAction {
    /// Show or change the current approval mode.
    Mode {
        #[command(subcommand)]
        action: ApprovalModeAction,
    },
    /// Manage allowlist rules.
    Rules {
        #[command(subcommand)]
        action: ApprovalRuleCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ApprovalModeAction {
    Show,
    Set { mode: ApprovalModeArg },
}

#[derive(Subcommand, Debug)]
pub enum ApprovalRuleCommand {
    List,
    Add {
        id: String,
        #[arg(long, value_enum)]
        action: ApprovalRuleActionArg,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long = "command")]
        commands: Vec<String>,
        #[arg(long = "path")]
        paths: Vec<String>,
        #[arg(long = "node")]
        nodes: Vec<String>,
        #[arg(long = "channel")]
        channels: Vec<String>,
        #[arg(long)]
        risk_tier: Option<u8>,
        #[arg(long)]
        effect: Option<String>,
    },
    Remove {
        id: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SecretBackendArg {
    Auto,
    Vault,
    Keychain,
    Env,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ApprovalModeArg {
    Default,
    Allowlist,
    Open,
    Assisted,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ApprovalRuleActionArg {
    Allow,
    #[value(name = "require-approval")]
    RequireApproval,
}

#[derive(Subcommand, Debug)]
pub enum McpAction {
    /// List configured MCP servers.
    List,
    /// Show one configured MCP server.
    Show { name: String },
    /// Install an MCP package.
    Install { source: String },
    /// Upgrade an installed MCP package from a package directory.
    Upgrade { source: String },
    /// Export a configured MCP server as a package skeleton.
    Export {
        /// Existing server selector: server name, plugin id, or plugin name.
        name: String,

        /// Directory to create for the exported package.
        dir: PathBuf,

        /// Optional human-readable package name for manifest.json.
        #[arg(long)]
        package_name: Option<String>,
    },
    /// Generate an MCP package skeleton for authors.
    Scaffold {
        /// Directory to create for the MCP package.
        dir: PathBuf,

        /// Human-readable package name.
        #[arg(long)]
        name: String,

        /// Template name to seed the package with.
        #[arg(long, default_value = "custom")]
        template: String,

        /// Stable MCP server name exposed at runtime.
        #[arg(long)]
        server_name: Option<String>,

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

        /// Additional allowed read path.
        #[arg(long = "read", value_name = "PATH")]
        read_paths: Vec<PathBuf>,

        /// Additional allowed write path.
        #[arg(long = "write", value_name = "PATH")]
        write_paths: Vec<PathBuf>,
    },
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
