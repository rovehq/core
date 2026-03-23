use serde::{Deserialize, Serialize};

use super::defaults::default_secret_backend;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SecretBackend {
    #[default]
    Auto,
    Vault,
    Keychain,
    Env,
}

impl SecretBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Vault => "vault",
            Self::Keychain => "keychain",
            Self::Env => "env",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsConfig {
    #[serde(default = "default_secret_backend")]
    pub backend: SecretBackend,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            backend: default_secret_backend(),
        }
    }
}
