use clap::ValueEnum;

#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TaskView {
    Clean,
    Live,
    Logs,
    Gist,
}

impl TaskView {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskView::Clean => "clean",
            TaskView::Live => "live",
            TaskView::Logs => "logs",
            TaskView::Gist => "gist",
        }
    }

    pub fn with_stream(self, stream: bool) -> Self {
        if stream && matches!(self, TaskView::Clean) {
            TaskView::Live
        } else {
            self
        }
    }

    pub fn wants_progress(self) -> bool {
        matches!(self, TaskView::Live | TaskView::Logs)
    }
}
