use crate::agent_handle::AgentHandle;
use crate::bus_handle::BusHandle;
use crate::config_handle::ConfigHandle;
use crate::crypto_handle::CryptoHandle;
use crate::db_handle::DbHandle;
use crate::errors::EngineError;
use crate::network_handle::NetworkHandle;
use crate::tool_io::{ToolInput, ToolOutput};

/// Trait that all native core tools must implement.
pub trait CoreTool: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn start(&mut self, ctx: CoreContext) -> Result<(), EngineError>;
    fn stop(&mut self) -> Result<(), EngineError>;
    fn handle(&self, input: ToolInput) -> Result<ToolOutput, EngineError>;
}

/// Context provided to core tools for engine interaction.
#[derive(Clone)]
pub struct CoreContext {
    pub agent: AgentHandle,
    pub db: DbHandle,
    pub config: ConfigHandle,
    pub crypto: CryptoHandle,
    pub network: NetworkHandle,
    pub bus: BusHandle,
}

impl CoreContext {
    pub fn new(
        agent: AgentHandle,
        db: DbHandle,
        config: ConfigHandle,
        crypto: CryptoHandle,
        network: NetworkHandle,
        bus: BusHandle,
    ) -> Self {
        Self {
            agent,
            db,
            config,
            crypto,
            network,
            bus,
        }
    }
}
