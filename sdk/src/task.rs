use serde::{Deserialize, Serialize};

/// Task source for authentication and authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskSource {
    Cli,
    Telegram(String),
    Channel(String),
    WebUI,
    Remote(String),
}

impl TaskSource {
    /// Convert to a database-safe string representation.
    pub fn as_str(&self) -> String {
        match self {
            Self::Cli => "cli".to_string(),
            Self::Telegram(user_id) => format!("telegram:{user_id}"),
            Self::Channel(kind) => format!("channel:{kind}"),
            Self::WebUI => "webui".to_string(),
            Self::Remote(device_id) => format!("remote:{device_id}"),
        }
    }

    /// Parse a task source from persisted storage.
    pub fn parse_str(s: &str) -> Self {
        if s == "cli" {
            Self::Cli
        } else if s == "webui" {
            Self::WebUI
        } else if let Some(user_id) = s.strip_prefix("telegram:") {
            Self::Telegram(user_id.to_string())
        } else if let Some(kind) = s.strip_prefix("channel:") {
            Self::Channel(kind.to_string())
        } else if let Some(device_id) = s.strip_prefix("remote:") {
            Self::Remote(device_id.to_string())
        } else {
            Self::Remote(s.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TaskSource;

    #[test]
    fn channel_task_source_roundtrips() {
        let source = TaskSource::Channel("discord".to_string());
        let persisted = source.as_str();
        assert_eq!(persisted, "channel:discord");
        assert_eq!(TaskSource::parse_str(&persisted), source);
    }
}
