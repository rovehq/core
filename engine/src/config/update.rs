use serde::{Deserialize, Serialize};

use super::channel::Channel;

/// Read-only view of the active update channel, surfaced in `rove config show`.
///
/// The active channel comes from the binary (build-time feature + `ROVE_CHANNEL`
/// runtime override). This struct is NOT the source of truth — it exists so
/// operators can inspect the channel alongside the rest of the config snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    #[serde(default = "default_channel_label")]
    pub channel: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            channel: default_channel_label(),
        }
    }
}

impl UpdateConfig {
    pub fn from_active() -> Self {
        Self {
            channel: Channel::current().as_str().to_string(),
        }
    }

    /// Overwrite the stored label with the actually-active channel. Called
    /// during config normalization so the persisted value can never drift from
    /// the running binary's channel.
    pub fn normalize(&mut self) {
        self.channel = Channel::current().as_str().to_string();
    }
}

fn default_channel_label() -> String {
    Channel::current().as_str().to_string()
}

#[cfg(test)]
mod tests {
    use super::{Channel, UpdateConfig};

    #[test]
    fn normalize_mirrors_runtime_channel() {
        let previous = std::env::var_os("ROVE_CHANNEL");
        std::env::set_var("ROVE_CHANNEL", "dev");
        let mut cfg = UpdateConfig {
            channel: "stable".to_string(),
        };
        cfg.normalize();
        assert_eq!(cfg.channel, "dev");
        assert_eq!(Channel::current(), Channel::Dev);

        match previous {
            Some(value) => std::env::set_var("ROVE_CHANNEL", value),
            None => std::env::remove_var("ROVE_CHANNEL"),
        }
    }
}
