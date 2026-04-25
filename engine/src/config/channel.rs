use std::fmt;

const BUILD_CHANNEL: &str = env!("ROVE_BUILD_CHANNEL");

/// Release channel baked into the engine binary at build time and overridable
/// at runtime via `ROVE_CHANNEL=dev|stable` (primarily for local testing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Dev,
}

impl Channel {
    pub fn current() -> Self {
        if let Some(value) = std::env::var_os("ROVE_CHANNEL") {
            if let Some(parsed) = Self::parse(value.to_string_lossy().as_ref()) {
                return parsed;
            }
        }
        Self::parse(BUILD_CHANNEL).unwrap_or(Self::Stable)
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dev" | "nightly" => Some(Self::Dev),
            "stable" | "release" => Some(Self::Stable),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Dev => "dev",
        }
    }

    pub fn build_channel() -> Self {
        Self::parse(BUILD_CHANNEL).unwrap_or(Self::Stable)
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{Channel, BUILD_CHANNEL};

    #[test]
    fn parse_accepts_known_aliases() {
        assert_eq!(Channel::parse("dev"), Some(Channel::Dev));
        assert_eq!(Channel::parse("NIGHTLY"), Some(Channel::Dev));
        assert_eq!(Channel::parse("stable"), Some(Channel::Stable));
        assert_eq!(Channel::parse("release"), Some(Channel::Stable));
        assert_eq!(Channel::parse("bogus"), None);
    }

    #[test]
    fn build_channel_matches_feature_flag() {
        // The build script stamps stable vs dev based on CARGO_FEATURE_CHANNEL_DEV.
        let expected = if cfg!(feature = "channel-dev") {
            Channel::Dev
        } else {
            Channel::Stable
        };
        assert_eq!(Channel::build_channel(), expected);
        assert!(matches!(BUILD_CHANNEL, "stable" | "dev"));
    }
}
