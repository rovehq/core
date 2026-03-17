//! WebSocket Client for External UI
//!
//! Rove connects outward as a WebSocket **client** to a remote server
//! (e.g. an external dashboard). The server can submit tasks; Rove
//! sends back results.
//!
//! Features:
//! - Auto-reconnect with configurable delay
//! - JSON message protocol (submit_task, ping/pong, task results)
//! - Optional auth_token sent on connect

use futures::stream::StreamExt;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{error, info, warn};

use crate::config::WsClientConfig;

/// Inbound message received from the remote server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundMessage {
    /// Server asks Rove to run a task.
    SubmitTask { task_id: String, input: String },
    /// Server ping (Rove replies with pong).
    Ping,
}

/// Outbound message sent by Rove to the remote server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundMessage {
    /// Sent immediately after connecting to authenticate.
    AuthHello {
        #[serde(skip_serializing_if = "Option::is_none")]
        auth_token: Option<String>,
    },
    /// Acknowledge that a task was accepted.
    TaskSubmitted { task_id: String },
    /// Task finished successfully.
    TaskCompleted { task_id: String, answer: String },
    /// Task failed.
    TaskFailed { task_id: String, error: String },
    /// Reply to a server ping.
    Pong,
}

/// A task received from the remote UI.
#[derive(Debug, Clone)]
pub struct RemoteTask {
    pub task_id: String,
    pub input: String,
}

/// Channel receiver for incoming remote tasks.
pub type TaskReceiver = mpsc::Receiver<RemoteTask>;

/// Channel sender for outbound results (used by the agent after completing a task).
pub type ResultSender = mpsc::Sender<OutboundMessage>;

/// Start the WebSocket client.
///
/// Spawns an auto-reconnect loop in the background.
/// Returns a receiver for incoming tasks and a sender for outbound results.
pub fn start(config: WsClientConfig) -> (TaskReceiver, ResultSender) {
    let (task_tx, task_rx) = mpsc::channel::<RemoteTask>(64);
    let (result_tx, result_rx) = mpsc::channel::<OutboundMessage>(64);

    tokio::spawn(reconnect_loop(config, task_tx, result_rx));

    (task_rx, result_tx)
}

/// Auto-reconnect loop. Keeps trying to maintain a connection.
async fn reconnect_loop(
    config: WsClientConfig,
    task_tx: mpsc::Sender<RemoteTask>,
    mut result_rx: mpsc::Receiver<OutboundMessage>,
) {
    loop {
        info!("WS client connecting to {}", config.url);

        match tokio_tungstenite::connect_async(&config.url).await {
            Ok((ws_stream, _response)) => {
                info!("WS client connected to {}", config.url);

                let (mut write, mut read) = ws_stream.split();

                // Send auth hello
                let hello = OutboundMessage::AuthHello {
                    auth_token: config.auth_token.clone(),
                };
                if let Ok(json) = serde_json::to_string(&hello) {
                    if let Err(e) = write.send(WsMessage::Text(json)).await {
                        warn!("Failed to send auth hello: {}", e);
                    }
                }

                // Run read/write until disconnect
                loop {
                    tokio::select! {
                        // Inbound from server
                        msg = read.next() => {
                            match msg {
                                Some(Ok(WsMessage::Text(text))) => {
                                    handle_inbound(&text, &task_tx, &mut write).await;
                                }
                                Some(Ok(WsMessage::Ping(data))) => {
                                    let _ = write.send(WsMessage::Pong(data)).await;
                                }
                                Some(Ok(WsMessage::Close(_))) | None => {
                                    info!("WS connection closed by server");
                                    break;
                                }
                                Some(Err(e)) => {
                                    warn!("WS read error: {}", e);
                                    break;
                                }
                                _ => {} // Binary, Pong, Frame — ignore
                            }
                        }
                        // Outbound results from agent
                        result = result_rx.recv() => {
                            match result {
                                Some(outbound) => {
                                    if let Ok(json) = serde_json::to_string(&outbound) {
                                        if let Err(e) = write.send(WsMessage::Text(json)).await {
                                            warn!("Failed to send outbound message: {}", e);
                                            break;
                                        }
                                    }
                                }
                                None => {
                                    // Result channel closed — shut down
                                    info!("Result channel closed, stopping WS client");
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("WS client failed to connect: {}", e);
            }
        }

        // Reconnect delay
        info!(
            "WS client reconnecting in {}s...",
            config.reconnect_delay_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(config.reconnect_delay_secs)).await;
    }
}

/// Handle a single inbound text message from the server.
async fn handle_inbound<S>(text: &str, task_tx: &mpsc::Sender<RemoteTask>, write: &mut S)
where
    S: SinkExt<WsMessage> + Unpin,
    S::Error: std::fmt::Display,
{
    let msg: InboundMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!("Failed to parse inbound WS message: {} — raw: {}", e, text);
            return;
        }
    };

    match msg {
        InboundMessage::SubmitTask { task_id, input } => {
            info!("Received remote task {}: {}", task_id, input);

            // Acknowledge
            let ack = OutboundMessage::TaskSubmitted {
                task_id: task_id.clone(),
            };
            if let Ok(json) = serde_json::to_string(&ack) {
                if let Err(e) = write.send(WsMessage::Text(json)).await {
                    warn!("Failed to send task_submitted ack: {}", e);
                }
            }

            // Forward to agent
            if let Err(e) = task_tx.send(RemoteTask { task_id, input }).await {
                error!("Failed to forward remote task to agent: {}", e);
            }
        }
        InboundMessage::Ping => {
            let pong = OutboundMessage::Pong;
            if let Ok(json) = serde_json::to_string(&pong) {
                if let Err(e) = write.send(WsMessage::Text(json)).await {
                    warn!("Failed to send pong: {}", e);
                }
            }
        }
    }
}
