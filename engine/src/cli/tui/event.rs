use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent};
use futures::StreamExt;
use tokio::sync::mpsc;

pub enum AppEvent {
    Key(CrosstermEvent),
    Tick,
}

pub fn spawn_event_reader(tx: mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut stream = event::EventStream::new();
        loop {
            tokio::select! {
                Some(Ok(ev)) = stream.next() => {
                    if tx.send(AppEvent::Key(ev)).is_err() {
                        break;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(200)) => {
                    if tx.send(AppEvent::Tick).is_err() {
                        break;
                    }
                }
            }
        }
    });
}
