//! Agent Loop Core
//!
//! This module implements the core agent loop that processes tasks through
//! an iterative think-act-observe cycle. The agent maintains conversation
//! history, assesses risk, and coordinates with LLM providers to execute tasks.

pub mod core;
pub mod preferences;
pub mod working_memory;

pub use core::{AgentCore, TaskResult};
pub use working_memory::WorkingMemory;
