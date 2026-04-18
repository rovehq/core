pub mod setup;

mod action;
mod app;
mod commands;
mod dispatch;
mod event;
mod streaming;
mod theme;
mod ui;
mod widgets;

pub use app::run_tui;
