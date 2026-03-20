use sdk::errors::EngineError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SDK_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub sdk_version: String,
    pub plugin_type: PluginType,
    pub permissions: Permissions,
    pub trust_tier: TrustTier,
    pub min_model: Option<ModelRequirement>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PluginType {
    Skill,
    Channel,
    Brain,
    Workspace,
    Mcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Permissions {
    pub filesystem: Vec<PathPattern>,
    pub network: Vec<DomainPattern>,
    pub memory_read: bool,
    pub memory_write: bool,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustTier {
    Official = 0,
    Reviewed = 1,
    Community = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRequirement {
    pub min_context_tokens: Option<u32>,
    pub requires_tool_calls: bool,
    pub requires_vision: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PathPattern(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainPattern(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ToolCatalog {
    #[serde(default)]
    pub tools: Vec<DeclaredTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeclaredTool {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub domains: Vec<String>,
}

impl Manifest {
    pub fn from_json(json: &str) -> Result<Self, EngineError> {
        let manifest: Self = serde_json::from_str(json).map_err(|error| {
            EngineError::Config(format!("Invalid plugin manifest JSON: {error}"))
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), EngineError> {
        if self.name.trim().is_empty() {
            return Err(EngineError::Config(
                "Plugin manifest is missing a name".to_string(),
            ));
        }

        if self.version.trim().is_empty() {
            return Err(EngineError::Config(format!(
                "Plugin '{}' is missing a version",
                self.name
            )));
        }

        if self.description.trim().is_empty() {
            return Err(EngineError::Config(format!(
                "Plugin '{}' is missing a description",
                self.name
            )));
        }

        self.ensure_sdk_compatible()
    }

    pub fn ensure_sdk_compatible(&self) -> Result<(), EngineError> {
        if self.sdk_version != SDK_VERSION {
            return Err(EngineError::Config(format!(
                "Plugin '{}' targets SDK version '{}' but engine requires '{}'",
                self.name, self.sdk_version, SDK_VERSION
            )));
        }

        Ok(())
    }

    pub fn validate_install_record(
        &self,
        plugin_type: &str,
        trust_tier: i64,
    ) -> Result<(), EngineError> {
        let expected_type = PluginType::parse(plugin_type)?;
        if self.plugin_type != expected_type {
            return Err(EngineError::Config(format!(
                "Plugin '{}' manifest type '{:?}' does not match installed_plugins type '{}'",
                self.name, self.plugin_type, plugin_type
            )));
        }

        let expected_tier = TrustTier::try_from(trust_tier)?;
        if self.trust_tier != expected_tier {
            return Err(EngineError::Config(format!(
                "Plugin '{}' manifest trust tier '{:?}' does not match installed_plugins trust tier '{}'",
                self.name, self.trust_tier, trust_tier
            )));
        }

        Ok(())
    }
}

impl ToolCatalog {
    pub fn from_json(raw: Option<&str>) -> Result<Self, EngineError> {
        match raw {
            Some(raw) if !raw.trim().is_empty() => serde_json::from_str(raw).map_err(|error| {
                EngineError::Config(format!("Invalid plugin runtime config JSON: {error}"))
            }),
            _ => Ok(Self::default()),
        }
    }
}

impl PluginType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skill => "Skill",
            Self::Channel => "Channel",
            Self::Brain => "Brain",
            Self::Workspace => "Workspace",
            Self::Mcp => "Mcp",
        }
    }

    pub fn parse(value: &str) -> Result<Self, EngineError> {
        match value {
            "Skill" => Ok(Self::Skill),
            "Channel" => Ok(Self::Channel),
            "Brain" => Ok(Self::Brain),
            "Workspace" => Ok(Self::Workspace),
            "Mcp" => Ok(Self::Mcp),
            other => Err(EngineError::Config(format!(
                "Unknown plugin type '{}'",
                other
            ))),
        }
    }
}

impl TrustTier {
    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

impl TryFrom<i64> for TrustTier {
    type Error = EngineError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Official),
            1 => Ok(Self::Reviewed),
            2 => Ok(Self::Community),
            other => Err(EngineError::Config(format!(
                "Unknown trust tier '{}'",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Manifest, PluginType, ToolCatalog, TrustTier, SDK_VERSION};

    #[test]
    fn parses_and_validates_manifest_json() {
        let manifest = Manifest::from_json(
            r#"{
                "name": "echo-skill",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Skill",
                "permissions": {
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Echo tool"
            }"#,
        )
        .expect("manifest");

        assert_eq!(manifest.name, "echo-skill");
        assert_eq!(manifest.plugin_type, PluginType::Skill);
        assert_eq!(manifest.trust_tier, TrustTier::Reviewed);
    }

    #[test]
    fn rejects_sdk_version_mismatch() {
        let error = Manifest::from_json(
            r#"{
                "name": "echo-skill",
                "version": "0.1.0",
                "sdk_version": "9.9.9",
                "plugin_type": "Skill",
                "permissions": {
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Echo tool"
            }"#,
        )
        .expect_err("sdk mismatch");

        assert!(error
            .to_string()
            .contains(&format!("engine requires '{}'", SDK_VERSION)));
    }

    #[test]
    fn rejects_install_record_mismatch() {
        let manifest = Manifest::from_json(
            r#"{
                "name": "echo-skill",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Skill",
                "permissions": {
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Echo tool"
            }"#,
        )
        .expect("manifest");

        let error = manifest
            .validate_install_record("Mcp", 2)
            .expect_err("mismatched install record");
        assert!(error
            .to_string()
            .contains("does not match installed_plugins type"));
    }

    #[test]
    fn parses_tool_catalog_from_runtime_config() {
        let catalog = ToolCatalog::from_json(Some(
            r#"{
                "tools": [
                    {
                        "name": "echo",
                        "description": "Echo the input",
                        "parameters": {"type":"object"},
                        "domains": ["all", "general"]
                    }
                ]
            }"#,
        ))
        .expect("catalog");

        assert_eq!(catalog.tools.len(), 1);
        assert_eq!(catalog.tools[0].name, "echo");
        assert_eq!(catalog.tools[0].domains, vec!["all", "general"]);
    }
}
