mod menu;
mod preset;
mod prompt;
mod result;
mod summary;
mod wizard;

pub use result::SetupResult;
pub use summary::print_summary;
pub use wizard::run_setup_wizard;

pub(super) const CYAN: &str = "\x1b[38;5;39m";
pub(super) const GREEN: &str = "\x1b[38;5;48m";
pub(super) const DIM: &str = "\x1b[38;5;240m";
pub(super) const BOLD: &str = "\x1b[1m";
pub(super) const YELLOW: &str = "\x1b[38;5;220m";
pub(super) const RESET: &str = "\x1b[0m";
