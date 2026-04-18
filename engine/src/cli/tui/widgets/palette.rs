use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};

use super::super::commands::{self, Cmd};

pub const PALETTE_MAX: usize = 10;

pub struct Palette {
    pub stack: Vec<(&'static str, &'static [Cmd])>,
    pub filter: String,
    pub selected: usize,
    pub matched: Vec<usize>,
}

impl Palette {
    pub fn new() -> Self {
        let matched = (0..commands::COMMANDS.len()).collect();
        Self {
            stack: vec![("", commands::COMMANDS)],
            filter: String::new(),
            selected: 0,
            matched,
        }
    }

    pub fn current_list(&self) -> &'static [Cmd] {
        self.stack.last().map(|s| s.1).unwrap_or(commands::COMMANDS)
    }

    pub fn parent_prefix(&self) -> String {
        self.stack
            .iter()
            .map(|(t, _)| *t)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn total_available(&self) -> usize {
        self.current_list().len()
    }

    pub fn refresh_matches(&mut self) {
        let list = self.current_list();
        if self.filter.is_empty() {
            self.matched = (0..list.len()).collect();
            return;
        }
        let mut buf = Vec::new();
        let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
        let pattern = Pattern::parse(&self.filter, CaseMatching::Ignore, Normalization::Smart);
        let mut scored: Vec<(usize, u32)> = list
            .iter()
            .enumerate()
            .filter_map(|(i, cmd)| {
                let haystack = Utf32Str::new(cmd.name, &mut buf);
                let score = pattern.score(haystack, &mut matcher)?;
                Some((i, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        self.matched = scored.into_iter().map(|(i, _)| i).collect();
        if self.selected >= self.matched.len() {
            self.selected = 0;
        }
    }

    pub fn matches(&self) -> Vec<&Cmd> {
        let list = self.current_list();
        self.matched.iter().map(|&i| &list[i]).collect()
    }

    pub fn display_matches(&self) -> Vec<&Cmd> {
        self.matches().into_iter().take(PALETTE_MAX).collect()
    }

    pub fn selected_cmd(&self) -> Option<&Cmd> {
        let m = self.display_matches();
        m.get(self.selected).copied()
    }

    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.refresh_matches();
        self.selected = 0;
    }

    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.refresh_matches();
        self.selected = 0;
    }

    pub fn move_up(&mut self) {
        let len = self.display_matches().len();
        if len == 0 {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        let len = self.display_matches().len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected + 1).min(len.saturating_sub(1));
    }

    pub fn full_command_for_selected(&self) -> String {
        let prefix = self.parent_prefix();
        if let Some(cmd) = self.selected_cmd() {
            if prefix.is_empty() {
                cmd.name.to_string()
            } else {
                format!("{} {}", prefix, cmd.name)
            }
        } else {
            prefix
        }
    }

    pub fn drill(&mut self) -> bool {
        if let Some(cmd) = self.selected_cmd() {
            if !cmd.subcommands.is_empty() {
                self.stack.push((cmd.name, cmd.subcommands));
                self.filter.clear();
                self.refresh_matches();
                self.selected = 0;
                return true;
            }
        }
        false
    }

    pub fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.filter.clear();
            self.refresh_matches();
            self.selected = 0;
            true
        } else {
            false
        }
    }
}
