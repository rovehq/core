//! Core types for the Policy System
//!
//! Maps directly to the TOML and Markdown schema specifications
//! for policy files.
//!
//! Historical `Skill*` names remain as aliases for compatibility.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Full policy configuration definition from a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFile {
    pub meta: SkillMeta,
    #[serde(default)]
    pub activation: SkillActivation,
    #[serde(default)]
    pub directives: SkillDirectives,
    #[serde(default)]
    pub hints: HashMap<String, String>,
    #[serde(default)]
    pub routing: SkillRouting,
    #[serde(default)]
    pub tools: SkillTools,
    #[serde(default)]
    pub memory: SkillMemory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,

    // Phase 3 Requirements: Extends allowing inheritance
    #[serde(default)]
    pub extends: Option<String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillActivation {
    #[serde(default)]
    pub manual: bool,
    #[serde(default)]
    pub auto_when: Vec<String>,
    #[serde(default)]
    pub conflicts_with: Vec<String>,
    #[serde(default = "default_priority")]
    pub priority: u16,

    // Phase 3 Requirements: Auto Activation fields
    #[serde(default)]
    pub apply_only_to: Vec<String>,
    #[serde(default)]
    pub auto_when_risk_tier: Option<u8>,
    #[serde(default)]
    pub auto_when_file_type: Vec<String>,
    #[serde(default)]
    pub auto_when_provider: Option<String>,
}

fn default_priority() -> u16 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillDirectives {
    #[serde(default)]
    pub system_prefix: String,
    #[serde(default)]
    pub system_suffix: String,
    #[serde(default)]
    pub per_stage: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillRouting {
    #[serde(default)]
    pub preferred_providers: Vec<String>,
    #[serde(default)]
    pub avoid_providers: Vec<String>,
    #[serde(default)]
    pub prefer_mode: Option<String>,
    #[serde(default)]
    pub always_verify: bool,
    #[serde(default)]
    pub min_score_threshold: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillTools {
    #[serde(default)]
    pub prefer: Vec<String>,
    #[serde(default)]
    pub suggest_after_code: Vec<String>,
    // Phase 3:
    #[serde(default)]
    pub per_plugin: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillMemory {
    #[serde(default)]
    pub auto_tag: Vec<String>,
    #[serde(default = "default_episodic_limit")]
    pub episodic_limit: usize,
}

fn default_episodic_limit() -> usize {
    3
}

/// Merged directives from all active skills (conflict-resolved)
#[derive(Debug, Clone, Default)]
pub struct MergedDirectives {
    /// Combined system prefix (higher priority first)
    pub system_prefix: String,
    /// Combined system suffix (higher priority first)
    pub system_suffix: String,
    /// Per-stage directives (higher priority wins per stage)
    pub per_stage: HashMap<String, String>,
    /// All auto-tags from active skills
    pub auto_tags: Vec<String>,
}

/// Routing preferences merged from active skills
#[derive(Debug, Clone, Default)]
pub struct RoutingPreferences {
    /// Preferred providers (from highest priority skill)
    pub preferred_providers: Vec<String>,
    /// Providers to avoid (union of all active)
    pub avoid_providers: Vec<String>,
    /// Preferred execution mode (from highest priority skill)
    pub prefer_mode: Option<String>,
    /// Whether to always verify (true if any active skill requires it)
    pub always_verify: bool,
    /// Minimum score threshold (strictest across all active)
    pub min_score_threshold: f32,
}

pub type PolicyFile = SkillFile;
pub type PolicyMeta = SkillMeta;
pub type PolicyActivation = SkillActivation;
pub type PolicyDirectives = SkillDirectives;
pub type PolicyRouting = SkillRouting;
pub type PolicyTools = SkillTools;
pub type PolicyMemory = SkillMemory;
