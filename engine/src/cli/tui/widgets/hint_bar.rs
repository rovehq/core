use ratatui::{layout::Rect, style::Style, text::Span, widgets::Paragraph, Frame};

use super::super::theme;
use crate::cli::tui::app::InputMode;

pub fn render(f: &mut Frame, area: Rect, mode: InputMode) {
    let hints = match mode {
        InputMode::Normal => "  q=quit  i/Tab=edit  ↑↓=scroll",
        InputMode::Editing => "  Enter=submit  Esc=normal  ↑↓=history  Ctrl+C=quit",
    };

    let line =
        ratatui::text::Line::from(vec![Span::styled(hints, Style::default().fg(theme::DIM))]);

    let para = Paragraph::new(line);
    f.render_widget(para, area);
}
