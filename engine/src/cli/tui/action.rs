use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, InputMode};

pub enum Action {
    Quit,
    SubmitInput,
    ChangeInputMode(InputMode),
    InputChar(char),
    Backspace,
    Delete,
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,
    HistoryUp,
    HistoryDown,
    ClearInput,
    ClearScreen,
    ScrollUp,
    ScrollDown,
    PaletteOpen,
    PaletteDrill,
    PaletteBack,
    PaletteChar(char),
    PaletteBackspace,
    TaskListUp,
    TaskListDown,
    TaskListSelect,
    TaskListClose,
    None,
}

pub fn map(event: Event, app: &App) -> Action {
    match event {
        Event::Key(key) => map_key(key, app),
        Event::Resize(_, _) => Action::None,
        _ => Action::None,
    }
}

fn map_key(key: KeyEvent, app: &App) -> Action {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let none_mod = key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT;

    if app.task_list.is_some() {
        return map_task_list_key(key, ctrl);
    }

    if app.palette.is_some() {
        return map_palette_key(key, ctrl, none_mod, shift);
    }

    match key.code {
        KeyCode::Char('c') if ctrl => Action::Quit,
        KeyCode::Char('l') if ctrl => Action::ClearScreen,
        KeyCode::Char('u') if ctrl => Action::ClearInput,
        KeyCode::Char('q') if ctrl && app.input_mode == InputMode::Normal => Action::Quit,

        KeyCode::Char('/')
            if app.input.buffer.is_empty() && app.input_mode == InputMode::Editing =>
        {
            Action::PaletteOpen
        }
        KeyCode::Char(c) if none_mod && app.input_mode == InputMode::Editing => {
            Action::InputChar(if shift { c.to_ascii_uppercase() } else { c })
        }
        KeyCode::Enter if app.input_mode == InputMode::Editing => Action::SubmitInput,
        KeyCode::Backspace if app.input_mode == InputMode::Editing => Action::Backspace,
        KeyCode::Delete if app.input_mode == InputMode::Editing => Action::Delete,
        KeyCode::Left if app.input_mode == InputMode::Editing && !ctrl => Action::CursorLeft,
        KeyCode::Right if app.input_mode == InputMode::Editing && !ctrl => Action::CursorRight,
        KeyCode::Home if app.input_mode == InputMode::Editing => Action::CursorHome,
        KeyCode::End if app.input_mode == InputMode::Editing => Action::CursorEnd,
        KeyCode::Up if app.input_mode == InputMode::Editing => Action::HistoryUp,
        KeyCode::Down if app.input_mode == InputMode::Editing => Action::HistoryDown,
        KeyCode::Esc => Action::ChangeInputMode(InputMode::Normal),

        KeyCode::Char('i') if ctrl || key.code == KeyCode::Tab => {
            Action::ChangeInputMode(InputMode::Editing)
        }

        KeyCode::Up if app.input_mode == InputMode::Normal => Action::ScrollUp,
        KeyCode::Down if app.input_mode == InputMode::Normal => Action::ScrollDown,

        _ => Action::None,
    }
}

fn map_palette_key(key: KeyEvent, ctrl: bool, none_mod: bool, shift: bool) -> Action {
    match key.code {
        KeyCode::Char('c') if ctrl => Action::Quit,
        KeyCode::Esc => Action::PaletteBack,
        KeyCode::Backspace => Action::PaletteBackspace,
        KeyCode::Up => Action::HistoryUp,
        KeyCode::Down => Action::HistoryDown,
        KeyCode::Right | KeyCode::Tab => Action::PaletteDrill,
        KeyCode::Left => Action::PaletteBack,
        KeyCode::Enter => Action::SubmitInput,
        KeyCode::Char(c) if none_mod => {
            Action::PaletteChar(if shift { c.to_ascii_uppercase() } else { c })
        }
        _ => Action::None,
    }
}

fn map_task_list_key(key: KeyEvent, ctrl: bool) -> Action {
    match key.code {
        KeyCode::Char('c') if ctrl => Action::TaskListClose,
        KeyCode::Esc => Action::TaskListClose,
        KeyCode::Up => Action::TaskListUp,
        KeyCode::Down => Action::TaskListDown,
        KeyCode::Enter => Action::TaskListSelect,
        KeyCode::Char('q') => Action::TaskListClose,
        _ => Action::None,
    }
}
