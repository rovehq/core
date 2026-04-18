use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use std::io::{self, Write};

use super::{GREEN, RESET};

#[allow(dead_code)]
pub fn select_menu(stdout: &mut io::Stdout, items: &[String]) -> Result<usize> {
    select_menu_default(stdout, items, 0)
}

pub fn select_menu_default(
    stdout: &mut io::Stdout,
    items: &[String],
    default: usize,
) -> Result<usize> {
    let mut selected = default;

    for (index, item) in items.iter().enumerate() {
        draw_item(stdout, index, selected, item)?;
    }
    stdout.flush()?;

    loop {
        if let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = event::read()?
        {
            if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
                terminal::disable_raw_mode()?;
                std::process::exit(0);
            }

            match code {
                KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    if selected + 1 < items.len() {
                        selected += 1;
                    }
                }
                KeyCode::Enter => break,
                _ => {}
            }

            write!(stdout, "\x1b[{}A", items.len())?;
            for (index, item) in items.iter().enumerate() {
                write!(stdout, "\r\x1b[2K")?;
                draw_item(stdout, index, selected, item)?;
            }
            stdout.flush()?;
        }
    }

    Ok(selected)
}

fn draw_item(stdout: &mut io::Stdout, index: usize, selected: usize, item: &str) -> Result<()> {
    if index == selected {
        write!(stdout, "    {GREEN}>{RESET} {item}\r\n")?;
    } else {
        write!(stdout, "      {item}\r\n")?;
    }
    Ok(())
}
