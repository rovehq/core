use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use std::io::{self, Write};

use super::{BOLD, DIM, RESET};

pub fn print_line(stdout: &mut io::Stdout, text: &str) -> Result<()> {
    write!(stdout, "{}\r\n", text)?;
    stdout.flush()?;
    Ok(())
}

pub fn prompt_text(stdout: &mut io::Stdout, label: &str, default: &str) -> Result<String> {
    if default.is_empty() {
        write!(stdout, "  {BOLD}{label}{RESET}: ")?;
    } else {
        write!(stdout, "  {BOLD}{label}{RESET} {DIM}[{default}]{RESET}: ")?;
    }
    stdout.flush()?;

    let mut input = String::new();

    loop {
        if let Event::Key(KeyEvent { code, modifiers, .. }) = event::read()? {
            if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
                terminal::disable_raw_mode()?;
                std::process::exit(0);
            }

            match code {
                KeyCode::Enter => {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    break;
                }
                KeyCode::Char(character) => {
                    input.push(character);
                    write!(stdout, "{}", character)?;
                    stdout.flush()?;
                }
                KeyCode::Backspace => {
                    if !input.is_empty() {
                        input.pop();
                        write!(stdout, "\x08 \x08")?;
                        stdout.flush()?;
                    }
                }
                _ => {}
            }
        }
    }

    let input = input.trim().to_string();
    if input.is_empty() && !default.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input)
    }
}
