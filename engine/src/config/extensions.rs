use serde::{Deserialize, Serialize};

use super::channel::Channel;

/// Operator-controlled settings for the extension/catalog fetch path.
///
/// `channel` is independent of the engine's release channel: dev-engine users
/// keep stable extensions by default, and can opt into the dev extension track
/// by setting `extensions.channel = "dev"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionsConfig {
    #[serde(default = "default_extensions_channel")]
    pub channel: String,
}

impl Default for ExtensionsConfig {
    fn default() -> Self {
        Self {
            channel: default_extensions_channel(),
        }
    }
}

impl ExtensionsConfig {
    pub fn normalize(&mut self) {
        self.channel = Channel::parse(&self.channel)
            .unwrap_or(Channel::Stable)
            .as_str()
            .to_string();
    }

    pub fn resolved_channel(&self) -> Channel {
        Channel::parse(&self.channel).unwrap_or(Channel::Stable)
    }
}

fn default_extensions_channel() -> String {
    Channel::Stable.as_str().to_string()
}

#[cfg(test)]
mod tests {
    use super::{Channel, ExtensionsConfig};

    #[test]
    fn defaults_to_stable_channel() {
        assert_eq!(ExtensionsConfig::default().resolved_channel(), Channel::Stable);
    }

    #[test]
    fn normalize_rejects_unknown_channel() {
        let mut cfg = ExtensionsConfig {
            channel: "nightly-42".to_string(),
        };
        cfg.normalize();
        assert_eq!(cfg.channel, "stable");
    }

    #[test]
    fn normalize_accepts_dev_alias() {
        let mut cfg = ExtensionsConfig {
            channel: "DEV".to_string(),
        };
        cfg.normalize();
        assert_eq!(cfg.channel, "dev");
        assert_eq!(cfg.resolved_channel(), Channel::Dev);
    }
}
