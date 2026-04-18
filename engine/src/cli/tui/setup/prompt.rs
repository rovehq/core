use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use std::io::{self, Write};

use super::{BOLD, DIM, RESET};

pub enum NavigationAction {
    Next,
    Back,
    Quit,
}

pub fn print_line(stdout: &mut io::Stdout, text: &str) -> Result<()> {
    write!(stdout, "{}\r\n", text)?;
    stdout.flush()?;
    Ok(())
}

#[allow(dead_code)]
pub fn prompt_text(stdout: &mut io::Stdout, label: &str, default: &str) -> Result<String> {
    if default.is_empty() {
        write!(stdout, "  {BOLD}{label}{RESET}: ")?;
    } else {
        write!(stdout, "  {BOLD}{label}{RESET} {DIM}[{default}]{RESET}: ")?;
    }
    stdout.flush()?;

    let mut input = String::new();

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

pub fn prompt_text_with_nav(
    stdout: &mut io::Stdout,
    label: &str,
    default: &str,
) -> Result<(Option<String>, NavigationAction)> {
    if default.is_empty() {
        write!(stdout, "  {BOLD}{label}{RESET}: ")?;
    } else {
        write!(stdout, "  {BOLD}{label}{RESET} {DIM}[{default}]{RESET}: ")?;
    }
    write!(stdout, "{DIM}(Tab=next, Shift+Tab=back){RESET}\r\n  ")?;
    stdout.flush()?;

    let mut input = String::new();

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
                KeyCode::Enter => {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    let value = input.trim().to_string();
                    let result = if value.is_empty() && !default.is_empty() {
                        default.to_string()
                    } else {
                        value
                    };
                    return Ok((Some(result), NavigationAction::Next));
                }
                KeyCode::Tab if modifiers.contains(KeyModifiers::SHIFT) => {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    return Ok((None, NavigationAction::Back));
                }
                KeyCode::BackTab => {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    return Ok((None, NavigationAction::Back));
                }
                KeyCode::Tab => {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    let value = input.trim().to_string();
                    let result = if value.is_empty() && !default.is_empty() {
                        default.to_string()
                    } else {
                        value
                    };
                    return Ok((Some(result), NavigationAction::Next));
                }
                KeyCode::Left if modifiers.contains(KeyModifiers::ALT) => {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    return Ok((None, NavigationAction::Back));
                }
                KeyCode::Right if modifiers.contains(KeyModifiers::ALT) => {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    let value = input.trim().to_string();
                    let result = if value.is_empty() && !default.is_empty() {
                        default.to_string()
                    } else {
                        value
                    };
                    return Ok((Some(result), NavigationAction::Next));
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
}

pub fn prompt_secret(_stdout: &mut io::Stdout, label: &str) -> Result<String> {
    terminal::disable_raw_mode()?;
    let result = rpassword::read_password_from_tty(Some(&format!("  {label}: ")));
    terminal::enable_raw_mode()?;
    let value = result?;
    Ok(value.trim().to_string())
}
