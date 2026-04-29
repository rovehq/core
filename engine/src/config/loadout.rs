use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::runtime::builtin::BuiltinSelection;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LoadoutConfig {
    #[serde(default)]
    pub builtins: Vec<String>,
    #[serde(default)]
    pub drivers: Vec<String>,
    #[serde(default)]
    pub plugins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLoadout {
    pub profile_name: String,
    pub loadout_name: String,
    pub builtins: BTreeSet<String>,
    pub drivers: Option<BTreeSet<String>>,
    pub plugins: Option<BTreeSet<String>>,
    pub browser_profile: Option<String>,
    pub brain_profile: Option<String>,
    pub approval_profile: Option<String>,
}

impl ResolvedLoadout {
    pub fn compatibility_default() -> Self {
        let builtins = ["filesystem", "terminal"]
            .into_iter()
            .map(str::to_string)
            .collect();
        Self {
            profile_name: "default".to_string(),
            loadout_name: "default".to_string(),
            builtins,
            drivers: None,
            plugins: None,
            browser_profile: None,
            brain_profile: None,
            approval_profile: None,
        }
    }

    pub fn allows_driver(&self, name: &str, id: &str) -> bool {
        self.drivers
            .as_ref()
            .map(|allowed| allowed.contains(name) || allowed.contains(id))
            .unwrap_or(true)
    }

    pub fn allows_plugin(&self, name: &str, id: &str) -> bool {
        self.plugins
            .as_ref()
            .map(|allowed| allowed.contains(name) || allowed.contains(id))
            .unwrap_or(true)
    }

    pub fn builtin_selection(&self) -> BuiltinSelection {
        BuiltinSelection {
            filesystem: self.builtins.contains("filesystem"),
            terminal: self.builtins.contains("terminal"),
        }
    }
}
