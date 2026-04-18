//! Message Bus for inter-component communication
//!
//! The MessageBus provides a pub/sub pattern for components to communicate
//! without tight coupling. It uses bounded channels to prevent unbounded
//! memory growth and supports both specific event subscriptions and global
//! "All" subscriptions.
//!
//! # Requirements
//! - 1.2: Engine SHALL provide a Message_Bus for all inter-component communication
//! - 1.3: Engine SHALL prevent direct communication between Core_Tools and Plugins
//! - 29.4: Engine SHALL use bounded channels to prevent unbounded memory growth

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// Channel buffer size for bounded channels
const CHANNEL_BUFFER_SIZE: usize = 100;

/// Event types that can be published on the message bus
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum EventType {
    /// Task has started execution
    TaskStarted,
    /// Task has completed successfully
    TaskCompleted,
    /// Task has failed
    TaskFailed,
    /// A tool was called
    ToolCalled,
    /// A live normalized task stream event
    TaskStream,
    /// Daemon has started
    DaemonStarted,
    /// Daemon is stopping
    DaemonStopping,
    /// Configuration has changed
    ConfigChanged,
    /// A plugin has crashed
    PluginCrashed,
    /// Subscribe to all event types
    All,
}

/// Events that can be published on the message bus
#[derive(Debug, Clone)]
pub enum Event {
    /// Task started with ID and input
    TaskStarted { task_id: String, input: String },
    /// Task completed with ID and result
    TaskCompleted { task_id: String, result: String },
    /// Task failed with ID and error
    TaskFailed { task_id: String, error: String },
    /// Tool called with name and arguments
    ToolCalled {
        tool: String,
        args: serde_json::Value,
    },
    /// Normalized task event emitted directly from the agent loop
    TaskStream {
        task_id: String,
        event: TaskStreamEvent,
    },
    /// Daemon started
    DaemonStarted,
    /// Daemon stopping
    DaemonStopping,
    /// Configuration changed
    ConfigChanged {
        key: String,
        old_val: serde_json::Value,
        new_val: serde_json::Value,
    },
    /// Plugin crashed
    PluginCrashed { plugin_id: String, error: String },
}

impl Event {
    /// Get the event type for this event
    pub fn event_type(&self) -> EventType {
        match self {
            Event::TaskStarted { .. } => EventType::TaskStarted,
            Event::TaskCompleted { .. } => EventType::TaskCompleted,
            Event::TaskFailed { .. } => EventType::TaskFailed,
            Event::ToolCalled { .. } => EventType::ToolCalled,
            Event::TaskStream { .. } => EventType::TaskStream,
            Event::DaemonStarted => EventType::DaemonStarted,
            Event::DaemonStopping => EventType::DaemonStopping,
            Event::ConfigChanged { .. } => EventType::ConfigChanged,
            Event::PluginCrashed { .. } => EventType::PluginCrashed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStreamEvent {
    pub id: String,
    pub task_id: String,
    pub phase: String,
    pub summary: String,
    pub detail: Option<String>,
    pub raw_event_type: Option<String>,
    pub tool_name: Option<String>,
    pub status: Option<String>,
    pub step_num: i64,
    pub domain: Option<String>,
    pub created_at: i64,
}

/// Message bus for pub/sub communication between components
///
/// The MessageBus allows components to subscribe to specific event types
/// or all events, and publish events to all subscribers. It uses bounded
/// channels to prevent unbounded memory growth.
pub struct MessageBus {
    /// Map of event types to lists of subscribers
    /// Each subscriber gets a bounded channel with CHANNEL_BUFFER_SIZE capacity
    channels: Arc<Mutex<HashMap<EventType, Vec<mpsc::Sender<Event>>>>>,
}

impl MessageBus {
    /// Create a new MessageBus
    pub fn new() -> Self {
        Self {
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Subscribe to a specific event type
    ///
    /// Returns a receiver that will receive events of the specified type.
    /// The channel is bounded with CHANNEL_BUFFER_SIZE capacity to prevent
    /// unbounded memory growth.
    ///
    /// # Arguments
    /// * `event_type` - The type of events to subscribe to, or EventType::All for all events
    ///
    /// # Returns
    /// A receiver that will receive events of the specified type
    pub async fn subscribe(&self, event_type: EventType) -> mpsc::Receiver<Event> {
        let (tx, rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);
        let mut channels = self.channels.lock().await;
        channels.entry(event_type).or_default().push(tx);
        rx
    }

    /// Publish an event to all subscribers
    ///
    /// The event is sent to all subscribers of the specific event type,
    /// as well as all subscribers of EventType::All. If a subscriber's
    /// channel is full or closed, the send will fail silently.
    ///
    /// # Arguments
    /// * `event` - The event to publish
    pub async fn publish(&self, event: Event) {
        let channels = self.channels.lock().await;
        let event_type = event.event_type();

        // Send to specific event type subscribers
        if let Some(subscribers) = channels.get(&event_type) {
            for tx in subscribers {
                // Ignore send errors (subscriber may have dropped receiver)
                let _ = tx.send(event.clone()).await;
            }
        }

        // Also send to "All" subscribers
        if let Some(subscribers) = channels.get(&EventType::All) {
            for tx in subscribers {
                let _ = tx.send(event.clone()).await;
            }
        }
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
