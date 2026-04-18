use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::super::theme;
use crate::cli::tui::app::InputMode;

pub struct InputState {
    pub buffer: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub saved_buffer: String,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_idx: None,
            saved_buffer: String::new(),
        }
    }

    pub fn insert(&mut self, c: char) {
        self.buffer.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn cursor_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
        }
    }

    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    pub fn cursor_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_idx = None;
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let new_idx = match self.history_idx {
            None => {
                self.saved_buffer = self.buffer.clone();
                self.history.len() - 1
            }
            Some(0) => return,
            Some(i) => i - 1,
        };
        self.history_idx = Some(new_idx);
        self.buffer = self.history[new_idx].clone();
        self.cursor = self.buffer.len();
    }

    pub fn history_down(&mut self) {
        match self.history_idx {
            None => {}
            Some(i) if i + 1 >= self.history.len() => {
                self.history_idx = None;
                self.buffer = self.saved_buffer.clone();
                self.cursor = self.buffer.len();
            }
            Some(i) => {
                let new_idx = i + 1;
                self.history_idx = Some(new_idx);
                self.buffer = self.history[new_idx].clone();
                self.cursor = self.buffer.len();
            }
        }
    }

    pub fn push_history(&mut self, line: String) {
        if !line.is_empty() {
            self.history.retain(|h| h != &line);
            self.history.push(line);
            if self.history.len() > 200 {
                self.history.remove(0);
            }
        }
        self.history_idx = None;
        self.saved_buffer.clear();
    }

    pub fn take(&mut self) -> String {
        let s = self.buffer.trim().to_string();
        self.push_history(s.clone());
        self.clear();
        s
    }
}

pub fn render(f: &mut Frame, area: Rect, input: &InputState, mode: InputMode) {
    let border_style = match mode {
        InputMode::Normal => Style::default().fg(theme::DIM),
        InputMode::Editing => Style::default().fg(theme::CYAN),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(
            " ❯ ",
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let style = match mode {
        InputMode::Normal => Style::default().fg(theme::DIM),
        InputMode::Editing => Style::default().fg(theme::WHITE),
    };

    let display = if input.buffer.is_empty() && mode == InputMode::Editing {
        "type a task or / for commands…".to_string()
    } else {
        input.buffer.clone()
    };

    let display_style = if input.buffer.is_empty() && mode == InputMode::Editing {
        Style::default().fg(theme::DIM)
    } else {
        style
    };

    let para = Paragraph::new(display).style(display_style);
    f.render_widget(para, inner);

    if mode == InputMode::Editing && !input.buffer.is_empty() {
        f.set_cursor_position((inner.x + input.cursor as u16, inner.y));
    }
}
