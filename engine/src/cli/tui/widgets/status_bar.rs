use std::time::Instant;

use ratatui::{layout::Rect, style::Style, text::Span, widgets::Paragraph, Frame};

use super::super::theme;
use crate::config::channel::Channel;

pub struct StatusBar {
    pub spinning: bool,
    pub tick: u8,
    pub start_time: Option<Instant>,
    pub update_available: Option<(String, String)>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            spinning: false,
            tick: 0,
            start_time: None,
            update_available: None,
        }
    }

    pub fn on_tick(&mut self) {
        if self.spinning {
            self.tick = (self.tick + 1) % 4;
        }
    }

    pub fn elapsed_text(&self) -> String {
        if let Some(start) = self.start_time {
            let secs = start.elapsed().as_secs_f32();
            format!("{:.1}s", secs)
        } else {
            String::new()
        }
    }

    pub fn set_update_available(&mut self, current: String, latest: String) {
        self.update_available = Some((current, latest));
    }

    pub fn clear_update_available(&mut self) {
        self.update_available = None;
    }
}

pub fn render(f: &mut Frame, area: Rect, bar: &StatusBar) {
    let spinner = match bar.spinning {
        true => match bar.tick {
            0 => "⠋",
            1 => "⠙",
            2 => "⠹",
            _ => "⠸",
        },
        false => "●",
    };

    let status_text = if bar.spinning {
        let elapsed = bar.elapsed_text();
        format!("responding… {}", elapsed)
    } else {
        "ready".to_string()
    };

    let channel = Channel::current();
    let channel_label = format!(" [{}] ", channel.as_str());

    let mut spans = vec![
        Span::styled(
            format!(" {} ", spinner),
            Style::default().fg(if bar.spinning {
                theme::YELLOW
            } else {
                theme::GREEN
            }),
        ),
        Span::styled(status_text, Style::default().fg(theme::DIM)),
        Span::styled(channel_label, Style::default().fg(theme::DIM)),
    ];

    if let Some((current, latest)) = bar.update_available.as_ref() {
        spans.push(Span::styled(
            format!("· update v{} → v{} (run `rove update`)", current, latest),
            Style::default().fg(theme::YELLOW),
        ));
    }

    let line = ratatui::text::Line::from(spans);
    let para = Paragraph::new(line);
    f.render_widget(para, area);
}
