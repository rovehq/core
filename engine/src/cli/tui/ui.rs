use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use super::app::{App, InputMode};
use super::theme;
use super::widgets;

pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    let palette_height = if app.palette.is_some() {
        palette_needed_height(&app.palette) + 1
    } else {
        0
    };

    let status_height = 1u16;
    let hint_height = 1u16;
    let input_height = 3u16;
    let transcript_height = size
        .height
        .saturating_sub(status_height + hint_height + input_height + palette_height);

    let chunks = Layout::vertical([
        Constraint::Length(transcript_height),
        Constraint::Length(palette_height),
        Constraint::Length(input_height),
        Constraint::Length(status_height),
        Constraint::Length(hint_height),
    ])
    .split(size);

    widgets::transcript::render(f, chunks[0], &mut app.transcript);

    if let Some(ref mut palette) = app.palette {
        render_palette(f, chunks[1], palette);
    }

    widgets::input::render(f, chunks[2], &app.input, app.input_mode);
    widgets::status_bar::render(f, chunks[3], &app.status_bar);
    widgets::hint_bar::render(f, chunks[4], app.input_mode);

    if let Some(ref list) = app.task_list {
        render_task_list(f, chunks[0], list);
    }
}

fn palette_needed_height(palette: &Option<widgets::palette::Palette>) -> u16 {
    let p = match palette {
        Some(p) => p,
        None => return 0,
    };
    let count = p.display_matches().len().min(widgets::palette::PALETTE_MAX) as u16;
    if count == 0 {
        return 2;
    }
    count + 1
}

fn render_palette(f: &mut Frame, area: Rect, palette: &mut widgets::palette::Palette) {
    if area.height == 0 {
        return;
    }
    f.render_widget(Clear, area);

    let matches = palette.display_matches();
    let show_count = matches.len().min(widgets::palette::PALETTE_MAX);
    let total = palette.total_available();
    let matched = palette.matches().len();

    let border = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::DIM))
        .title(Span::styled(
            format!(" commands ({}/{}) ", matched, total),
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " ↑↓ navigate  Tab/→ drill  ← back  Enter run  Esc close ",
            Style::default().fg(theme::DIM),
        ));

    let inner = border.inner(area);
    f.render_widget(border, area);

    if show_count == 0 {
        let line = ratatui::text::Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("(no matching commands)", Style::default().fg(theme::DIM)),
        ]);
        let para = Paragraph::new(line);
        f.render_widget(para, inner);
        return;
    }

    let name_w = matches
        .iter()
        .map(|c| c.name.chars().count())
        .max()
        .unwrap_or(8)
        .clamp(8, 24);

    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let is_sel = i == palette.selected;
            let icon = if !cmd.subcommands.is_empty() {
                "▸"
            } else {
                " "
            };
            let name = format!("{:<width$}", cmd.name, width = name_w);
            let desc = cmd.description;
            let hint = if cmd.args_hint.is_empty() {
                String::new()
            } else {
                format!(" {}", cmd.args_hint)
            };

            let fg = if is_sel { theme::YELLOW } else { theme::DIM };
            let name_fg = if is_sel { theme::YELLOW } else { theme::WHITE };

            let mut spans = vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    icon,
                    Style::default().fg(if is_sel { theme::CYAN } else { theme::DIM }),
                ),
                Span::styled(" ", Style::default()),
                Span::styled(name, Style::default().fg(name_fg)),
                Span::styled(hint, Style::default().fg(fg)),
                Span::styled(format!(" {}", desc), Style::default().fg(fg)),
            ];

            if !cmd.subcommands.is_empty() {
                spans.push(Span::styled(
                    " →",
                    Style::default().fg(if is_sel { theme::CYAN } else { theme::DIM }),
                ));
            }

            ListItem::new(ratatui::text::Line::from(spans))
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::NONE));
    let mut state = ListState::default();
    state.select(Some(palette.selected.min(show_count.saturating_sub(1))));
    f.render_stateful_widget(list, inner, &mut state);

    if matched > widgets::palette::PALETTE_MAX {
        let more = matched - widgets::palette::PALETTE_MAX;
        let line = ratatui::text::Line::from(vec![Span::styled(
            format!("  … {} more", more),
            Style::default().fg(theme::DIM),
        )]);
        let para = Paragraph::new(line);
        let more_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        f.render_widget(para, more_area);
    }
}

use super::app::TaskListState;

fn render_task_list(f: &mut Frame, area: Rect, state: &TaskListState) {
    if state.tasks.is_empty() {
        return;
    }

    let width = (area.width.min(64)).max(30);
    let height = (state.tasks.len() as u16 + 2).min(area.height.saturating_sub(2));

    let list_area = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width,
        height,
    };

    f.render_widget(Clear, list_area);

    let border = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::CYAN))
        .title(Span::styled(
            " select task ",
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " ↑↓ select  Enter replay  Esc close ",
            Style::default().fg(theme::DIM),
        ));

    let inner = border.inner(list_area);
    f.render_widget(border, list_area);

    let items: Vec<ListItem> = state
        .tasks
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let is_sel = i == state.selected;
            let icon = if is_sel { "▶" } else { " " };
            let status_icon = match task.status.as_str() {
                "done" => "✓",
                "failed" => "✗",
                "running" => "⋯",
                _ => "·",
            };
            let status_fg = match task.status.as_str() {
                "done" => theme::GREEN,
                "failed" => theme::RED,
                "running" => theme::CYAN,
                _ => theme::DIM,
            };

            ListItem::new(ratatui::text::Line::from(vec![
                Span::styled(
                    format!("{} ", icon),
                    Style::default().fg(if is_sel { theme::YELLOW } else { theme::DIM }),
                ),
                Span::styled(status_icon, Style::default().fg(status_fg)),
                Span::styled(" ", Style::default()),
                Span::styled(
                    truncate_str(&task.preview, inner.width as usize - 8),
                    Style::default().fg(if is_sel { theme::YELLOW } else { theme::WHITE }),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::NONE));
    let mut list_state = ListState::default();
    list_state.select(Some(
        state.selected.min(state.tasks.len().saturating_sub(1)),
    ));
    f.render_stateful_widget(list, inner, &mut list_state);
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let t: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", t)
}
