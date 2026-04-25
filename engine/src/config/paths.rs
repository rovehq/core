use std::path::PathBuf;

use super::channel::Channel;

/// Root directory for all Rove on-disk state (config, data, logs, policy, hooks).
///
/// Resolution order:
/// 1. `ROVE_HOME` environment variable (absolute path).
/// 2. `$HOME/.rove-dev` when the active channel is Dev.
/// 3. `$HOME/.rove`.
/// 4. Current working directory fallback (`.rove`) if the home directory is
///    not discoverable.
pub fn rove_home() -> PathBuf {
    if let Some(path) = std::env::var_os("ROVE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }

    let leaf = match Channel::current() {
        Channel::Dev => ".rove-dev",
        Channel::Stable => ".rove",
    };

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(leaf)
}

#[cfg(test)]
mod tests {
    use super::rove_home;
    use std::path::PathBuf;
    use std::sync::Mutex;

    // Serialize tests that mutate process-global env vars so they don't
    // interfere with other tests (notably anything that reads `rove_home`
    // or `Channel::current` while the env is being swapped).
    fn env_guard() -> &'static Mutex<()> {
        static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn rove_home_respects_env_override() {
        let _guard = env_guard().lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("ROVE_HOME");
        std::env::set_var("ROVE_HOME", "/tmp/rove-home-override");
        assert_eq!(rove_home(), PathBuf::from("/tmp/rove-home-override"));
        match previous {
            Some(value) => std::env::set_var("ROVE_HOME", value),
            None => std::env::remove_var("ROVE_HOME"),
        }
    }

    #[test]
    fn rove_home_defaults_per_channel() {
        let _guard = env_guard().lock().unwrap_or_else(|e| e.into_inner());
        let previous_home = std::env::var_os("ROVE_HOME");
        let previous_channel = std::env::var_os("ROVE_CHANNEL");
        std::env::remove_var("ROVE_HOME");

        std::env::set_var("ROVE_CHANNEL", "stable");
        let stable = rove_home();
        assert!(stable.ends_with(".rove"));

        std::env::set_var("ROVE_CHANNEL", "dev");
        let dev = rove_home();
        assert!(dev.ends_with(".rove-dev"));

        match previous_home {
            Some(value) => std::env::set_var("ROVE_HOME", value),
            None => std::env::remove_var("ROVE_HOME"),
        }
        match previous_channel {
            Some(value) => std::env::set_var("ROVE_CHANNEL", value),
            None => std::env::remove_var("ROVE_CHANNEL"),
        }
    }
}
