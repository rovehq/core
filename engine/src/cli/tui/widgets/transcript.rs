use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::config::metadata::{APP_DISPLAY_NAME, VERSION};

use super::super::theme;

pub struct Transcript {
    pub messages: Vec<ListItem<'static>>,
    pub state: ListState,
}

impl Transcript {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(None);
        Self {
            messages: Self::welcome_lines(),
            state,
        }
    }

    fn welcome_lines() -> Vec<ListItem<'static>> {
        let heading = format!("{} v{}", APP_DISPLAY_NAME, VERSION);
        let hint = "Type a task, or / for commands   Ctrl+C to exit";
        vec![
            ListItem::new(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    format!("  {} ", heading),
                    Style::default().fg(theme::BLUE),
                ),
                ratatui::text::Span::styled("daemon: running", Style::default().fg(theme::GREEN)),
            ])),
            ListItem::new(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled("  ", Style::default()),
                ratatui::text::Span::styled(hint, Style::default().fg(theme::DIM)),
            ])),
            ListItem::new(ratatui::text::Line::from("")),
        ]
    }

    pub fn push(&mut self, line: ratatui::text::Line<'static>) {
        self.messages.push(ListItem::new(line));
        let len = self.messages.len();
        self.state.select(Some(len.saturating_sub(1)));
    }

    pub fn scroll_up(&mut self) {
        if let Some(i) = self.state.selected() {
            self.state.select(Some(i.saturating_sub(1)));
        }
    }

    pub fn scroll_down(&mut self) {
        let max = self.messages.len().saturating_sub(1);
        if let Some(i) = self.state.selected() {
            if i < max {
                self.state.select(Some(i + 1));
            }
        }
    }
}

pub fn render(f: &mut Frame, area: Rect, transcript: &mut Transcript) {
    let list =
        List::new(transcript.messages.clone()).block(Block::default().borders(Borders::NONE));
    f.render_stateful_widget(list, area, &mut transcript.state);
}
