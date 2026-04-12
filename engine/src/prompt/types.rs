use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct RevoConfig {
    pub agent: AgentMeta,
    pub modules: HashMap<String, String>,
    pub routing: HashMap<String, Route>,
}

#[derive(Debug, Deserialize)]
pub struct AgentMeta {
    pub name: String,
    pub version: String,
    pub codename: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct Route {
    pub description: String,
    pub load: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModuleContext {
    #[serde(flatten)]
    pub sections: HashMap<String, toml::Value>,
}
