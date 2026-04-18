use std::io;
use std::time::Instant;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use tokio::sync::mpsc;

use super::action::{self, Action};
use super::dispatch;
use super::event::{self, AppEvent};
use super::streaming;
use super::ui;
use super::widgets;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
}

pub struct TaskEntry {
    pub id: String,
    pub status: String,
    pub preview: String,
}

pub struct TaskListState {
    pub tasks: Vec<TaskEntry>,
    pub selected: usize,
}

pub enum DispatchResult {
    Output { _cmd: String, lines: Vec<String> },
    TaskOutput { _prompt: String, lines: Vec<String> },
    Error { _cmd: String, error: String },
    TaskList { tasks: Vec<TaskEntry> },
}

pub struct App {
    pub should_quit: bool,
    pub input_mode: InputMode,
    pub transcript: widgets::transcript::Transcript,
    pub input: widgets::input::InputState,
    pub status_bar: widgets::status_bar::StatusBar,
    pub palette: Option<widgets::palette::Palette>,
    pub task_list: Option<TaskListState>,
}

impl App {
    pub fn new() -> Self {
        Self {
            should_quit: false,
            input_mode: InputMode::Editing,
            transcript: widgets::transcript::Transcript::new(),
            input: widgets::input::InputState::new(),
            status_bar: widgets::status_bar::StatusBar::new(),
            palette: None,
            task_list: None,
        }
    }

    pub fn apply(&mut self, action: Action) -> Vec<DispatchReq> {
        let mut reqs = Vec::new();

        if self.task_list.is_some() {
            return self.apply_task_list_action(action, reqs);
        }

        match action {
            Action::Quit => self.should_quit = true,

            Action::PaletteOpen => {
                self.input.insert('/');
                self.palette = Some(widgets::palette::Palette::new());
            }
            Action::PaletteChar(c) => {
                if let Some(ref mut p) = self.palette {
                    p.push_filter_char(c);
                }
            }
            Action::PaletteBackspace => {
                if let Some(ref mut p) = self.palette {
                    if !p.filter.is_empty() {
                        p.pop_filter_char();
                    } else if p.stack.len() > 1 {
                        p.back();
                    } else {
                        self.palette = None;
                        self.input.clear();
                    }
                }
            }
            Action::PaletteDrill => {
                if let Some(ref mut p) = self.palette {
                    if !p.drill() {
                        let cmd = p.full_command_for_selected();
                        let has_sub = p
                            .selected_cmd()
                            .map(|c| !c.subcommands.is_empty())
                            .unwrap_or(false);
                        let args_hint = p.selected_cmd().map(|c| c.args_hint).unwrap_or("");
                        if !has_sub && !args_hint.is_empty() {
                            self.input.buffer = format!("/{} ", cmd);
                        } else {
                            self.input.buffer = format!("/{}", cmd);
                        }
                        self.input.cursor = self.input.buffer.len();
                        self.palette = None;
                    }
                }
            }
            Action::PaletteBack => {
                if let Some(ref mut p) = self.palette {
                    if !p.back() {
                        self.palette = None;
                        self.input.clear();
                    }
                }
            }

            Action::SubmitInput => {
                if self.palette.is_some() {
                    if let Some(ref p) = self.palette {
                        let cmd = p.full_command_for_selected();
                        let has_sub = p
                            .selected_cmd()
                            .map(|c| !c.subcommands.is_empty())
                            .unwrap_or(false);
                        if has_sub {
                            if let Some(ref mut pm) = self.palette {
                                pm.drill();
                            }
                            return reqs;
                        }
                        self.input.buffer = format!("/{}", cmd);
                        self.input.cursor = self.input.buffer.len();
                    }
                    self.palette = None;
                }
                let line = self.input.take();
                if line.is_empty() {
                    return reqs;
                }
                self.transcript.push(ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(
                        "  ❯ ",
                        ratatui::style::Style::default().fg(super::theme::BLUE),
                    ),
                    ratatui::text::Span::styled(
                        line.clone(),
                        ratatui::style::Style::default().fg(super::theme::CYAN),
                    ),
                ]));

                if line == "/quit" || line == "/exit" || line == "/q" {
                    self.should_quit = true;
                } else if line == "/help" {
                    self.push_help();
                } else if line == "/replay" {
                    reqs.push(DispatchReq::TaskList);
                } else if line.starts_with('/') {
                    let cmd_str = line.trim_start_matches('/').to_string();
                    reqs.push(DispatchReq::Command { cmd: cmd_str });
                } else {
                    reqs.push(DispatchReq::Task { prompt: line });
                }
            }
            Action::ChangeInputMode(mode) => self.input_mode = mode,
            Action::InputChar(c) => self.input.insert(c),
            Action::Backspace => self.input.backspace(),
            Action::Delete => self.input.delete(),
            Action::CursorLeft => self.input.cursor_left(),
            Action::CursorRight => self.input.cursor_right(),
            Action::CursorHome => self.input.cursor_home(),
            Action::CursorEnd => self.input.cursor_end(),
            Action::HistoryUp => {
                if self.palette.is_some() {
                    if let Some(ref mut p) = self.palette {
                        p.move_up();
                    }
                } else {
                    self.input.history_up();
                }
            }
            Action::HistoryDown => {
                if self.palette.is_some() {
                    if let Some(ref mut p) = self.palette {
                        p.move_down();
                    }
                } else {
                    self.input.history_down();
                }
            }
            Action::ClearInput => {
                self.input.clear();
                self.palette = None;
            }
            Action::ClearScreen => {}
            Action::ScrollUp => self.transcript.scroll_up(),
            Action::ScrollDown => self.transcript.scroll_down(),
            Action::TaskListUp
            | Action::TaskListDown
            | Action::TaskListSelect
            | Action::TaskListClose => {}
            Action::None => {}
        }
        reqs
    }

    fn apply_task_list_action(
        &mut self,
        action: Action,
        mut reqs: Vec<DispatchReq>,
    ) -> Vec<DispatchReq> {
        match action {
            Action::Quit | Action::TaskListClose => {
                self.task_list = None;
            }
            Action::TaskListUp => {
                if let Some(ref mut tl) = self.task_list {
                    tl.selected = tl.selected.saturating_sub(1);
                }
            }
            Action::TaskListDown => {
                if let Some(ref mut tl) = self.task_list {
                    tl.selected = (tl.selected + 1).min(tl.tasks.len().saturating_sub(1));
                }
            }
            Action::TaskListSelect => {
                if let Some(ref tl) = self.task_list {
                    if let Some(task) = tl.tasks.get(tl.selected) {
                        let task_id = task.id.clone();
                        self.task_list = None;
                        reqs.push(DispatchReq::Command {
                            cmd: format!("replay {}", task_id),
                        });
                    }
                }
            }
            _ => {}
        }
        reqs
    }

    pub fn on_tick(&mut self) {
        self.status_bar.on_tick();
    }

    pub fn apply_dispatch_result(&mut self, result: DispatchResult) {
        self.status_bar.spinning = false;
        self.status_bar.start_time = None;

        match result {
            DispatchResult::Output { lines, .. } | DispatchResult::TaskOutput { lines, .. } => {
                for line in lines {
                    if line.is_empty() {
                        self.transcript
                            .push(ratatui::text::Line::from(ratatui::text::Span::raw("")));
                    } else {
                        self.transcript.push(ratatui::text::Line::from(
                            ratatui::text::Span::styled(
                                format!("  {}", line),
                                ratatui::style::Style::default().fg(super::theme::WHITE),
                            ),
                        ));
                    }
                }
            }
            DispatchResult::Error { error, .. } => {
                self.transcript
                    .push(ratatui::text::Line::from(ratatui::text::Span::styled(
                        format!("  ✗ {}", error),
                        ratatui::style::Style::default().fg(super::theme::RED),
                    )));
            }
            DispatchResult::TaskList { tasks } => {
                if tasks.is_empty() {
                    self.transcript
                        .push(ratatui::text::Line::from(ratatui::text::Span::styled(
                            "  No tasks in history",
                            ratatui::style::Style::default().fg(super::theme::DIM),
                        )));
                } else {
                    self.task_list = Some(TaskListState { tasks, selected: 0 });
                }
            }
        }

        self.transcript
            .push(ratatui::text::Line::from(ratatui::text::Span::raw("")));
    }

    fn push_echo(&mut self, text: String) {
        self.transcript
            .push(ratatui::text::Line::from(ratatui::text::Span::styled(
                text,
                ratatui::style::Style::default().fg(super::theme::DIM),
            )));
    }

    fn push_help(&mut self) {
        let lines = [
            ("  /", "             open the command palette"),
            ("  /status", "  show daemon and environment status"),
            ("  /history", "  show recent task history"),
            ("  /replay", "  select a task to replay"),
            ("  /model list", "  list configured LLM providers"),
            ("  /memory status", "  show memory mode and graph health"),
            ("  /help", "  show this help"),
            ("  /quit", "  exit interactive mode"),
            ("", ""),
            (
                "  Ctrl+C",
                "   exit  │  Ctrl+L  clear screen  │  Ctrl+U  clear line",
            ),
            ("  ↑ ↓", "   history  │  ← → cursor  │  Tab  complete/drill"),
        ];
        for (key, val) in &lines {
            if key.is_empty() {
                self.push_echo(val.to_string());
            } else {
                self.transcript.push(ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(
                        key.to_string(),
                        ratatui::style::Style::default().fg(super::theme::CYAN),
                    ),
                    ratatui::text::Span::raw(val.to_string()),
                ]));
            }
        }
    }
}

pub enum DispatchReq {
    Command { cmd: String },
    Task { prompt: String },
    TaskList,
}

pub async fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AppEvent>();
    event::spawn_event_reader(event_tx.clone());

    let (dispatch_tx, mut dispatch_rx) = mpsc::unbounded_channel::<DispatchResult>();

    let mut app = App::new();

    loop {
        terminal.draw(|f| ui::render(f, &mut app))?;

        tokio::select! {
            Some(ev) = event_rx.recv() => {
                match ev {
                    AppEvent::Key(crossterm_ev) => {
                        let action = action::map(crossterm_ev, &app);
                        let reqs = app.apply(action);
                        for req in reqs {
                            app.status_bar.spinning = true;
                            app.status_bar.start_time = Some(Instant::now());
                            let tx = dispatch_tx.clone();
                            tokio::spawn(async move {
                                let result = run_dispatch(req).await;
                                let _ = tx.send(result);
                            });
                        }
                    }
                    AppEvent::Tick => app.on_tick(),
                }
            }
            Some(result) = dispatch_rx.recv() => {
                app.apply_dispatch_result(result);
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    println!("\r\n  Goodbye.\r\n");

    Ok(())
}

async fn run_dispatch(req: DispatchReq) -> DispatchResult {
    match req {
        DispatchReq::Command { cmd } => match dispatch::run(&cmd).await {
            Ok(lines) => DispatchResult::Output { _cmd: cmd, lines },
            Err(e) => DispatchResult::Error {
                _cmd: cmd,
                error: e.to_string(),
            },
        },
        DispatchReq::Task { prompt } => match streaming::run_task(&prompt).await {
            Ok(lines) => DispatchResult::TaskOutput {
                _prompt: prompt,
                lines,
            },
            Err(e) => DispatchResult::Error {
                _cmd: prompt,
                error: e.to_string(),
            },
        },
        DispatchReq::TaskList => match fetch_task_list().await {
            Ok(tasks) => DispatchResult::TaskList { tasks },
            Err(e) => DispatchResult::Error {
                _cmd: "replay".into(),
                error: e.to_string(),
            },
        },
    }
}

async fn fetch_task_list() -> anyhow::Result<Vec<TaskEntry>> {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("rove"));
    let output = tokio::process::Command::new(&exe)
        .args(["history", "--limit", "30"])
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut tasks = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("Task History")
            || line.starts_with('#')
            || line.starts_with("─")
        {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, char::is_whitespace).collect::<Vec<&str>>();
        let parts: Vec<&str> = parts.into_iter().filter(|p| !p.is_empty()).collect();
        if parts.len() >= 3 {
            tasks.push(TaskEntry {
                id: parts[0].to_string(),
                status: parts[1].to_string(),
                preview: parts[2..].join(" "),
            });
        } else if parts.len() == 2 {
            tasks.push(TaskEntry {
                id: parts[0].to_string(),
                status: parts[1].to_string(),
                preview: String::new(),
            });
        }
    }

    Ok(tasks)
}
