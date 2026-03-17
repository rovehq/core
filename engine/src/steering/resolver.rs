//! Skill Resolver
//!
//! Handles complex inheritance (extends), conflict resolution, and topological
//! sorting of Agent skills.

use super::types::SkillFile;
use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use tracing::warn;

/// Resolves `extends` properties by merging a parent skill into a child skill.
/// The child's properties override the parent's where there are conflicts.
pub fn resolve_inheritance(child: &mut SkillFile, parent: &SkillFile) -> Result<()> {
    // Merge Tags
    for tag in &parent.meta.tags {
        if !child.meta.tags.contains(tag) {
            child.meta.tags.push(tag.clone());
        }
    }

    // Merge Activation Settings
    if !child.activation.manual {
        child.activation.manual = parent.activation.manual;
    }

    // Union arrays
    extend_unique(
        &mut child.activation.auto_when,
        &parent.activation.auto_when,
    );
    extend_unique(
        &mut child.activation.conflicts_with,
        &parent.activation.conflicts_with,
    );
    extend_unique(
        &mut child.activation.apply_only_to,
        &parent.activation.apply_only_to,
    );
    extend_unique(
        &mut child.activation.auto_when_file_type,
        &parent.activation.auto_when_file_type,
    );

    // Take parent value if child didn't specify
    if child.activation.auto_when_risk_tier.is_none() {
        child.activation.auto_when_risk_tier = parent.activation.auto_when_risk_tier;
    }
    if child.activation.auto_when_provider.is_none() {
        child.activation.auto_when_provider = parent.activation.auto_when_provider.clone();
    }

    // Directives
    if child.directives.system_prefix.is_empty() {
        child.directives.system_prefix = parent.directives.system_prefix.clone();
    } else if !parent.directives.system_prefix.is_empty() {
        child.directives.system_prefix = format!(
            "{}\n\n{}",
            parent.directives.system_prefix, child.directives.system_prefix
        );
    }

    if child.directives.system_suffix.is_empty() {
        child.directives.system_suffix = parent.directives.system_suffix.clone();
    } else if !parent.directives.system_suffix.is_empty() {
        child.directives.system_suffix = format!(
            "{}\n\n{}",
            child.directives.system_suffix, parent.directives.system_suffix
        );
    }

    // Merge maps (child overwrites parent keys)
    for (stage, directive) in &parent.directives.per_stage {
        if !child.directives.per_stage.contains_key(stage) {
            child
                .directives
                .per_stage
                .insert(stage.clone(), directive.clone());
        }
    }

    // Routing
    extend_unique(
        &mut child.routing.preferred_providers,
        &parent.routing.preferred_providers,
    );
    extend_unique(
        &mut child.routing.avoid_providers,
        &parent.routing.avoid_providers,
    );

    if child.routing.prefer_mode.is_none() {
        child.routing.prefer_mode = parent.routing.prefer_mode.clone();
    }
    // Only upgrade verify to true
    if parent.routing.always_verify {
        child.routing.always_verify = true;
    }

    // Choose strictest score
    if let Some(p_score) = parent.routing.min_score_threshold {
        if let Some(c_score) = child.routing.min_score_threshold {
            if p_score > c_score {
                child.routing.min_score_threshold = Some(p_score);
            }
        } else {
            child.routing.min_score_threshold = Some(p_score);
        }
    }

    // Tools
    extend_unique(&mut child.tools.prefer, &parent.tools.prefer);
    extend_unique(
        &mut child.tools.suggest_after_code,
        &parent.tools.suggest_after_code,
    );

    for (plugin, instructions) in &parent.tools.per_plugin {
        if !child.tools.per_plugin.contains_key(plugin) {
            child
                .tools
                .per_plugin
                .insert(plugin.clone(), instructions.clone());
        }
    }

    // Memory
    extend_unique(&mut child.memory.auto_tag, &parent.memory.auto_tag);

    Ok(())
}

fn extend_unique(target: &mut Vec<String>, source: &[String]) {
    for s in source {
        if !target.contains(s) {
            target.push(s.clone());
        }
    }
}

/// Topologically sorts skills to resolve `extends` inheritance in the right order.
pub fn build_inheritance_graph(skills: &mut HashMap<String, super::loader::Skill>) -> Result<()> {
    // 1. Build adjacency list child -> parent
    let mut dependencies: HashMap<String, String> = HashMap::new();
    let mut all_nodes: HashSet<String> = HashSet::new();

    for (id, skill) in skills.iter() {
        all_nodes.insert(id.clone());
        if let Some(cfg) = &skill.config {
            if let Some(parent_id) = &cfg.meta.extends {
                let pid_lower = parent_id.to_lowercase();
                dependencies.insert(id.clone(), pid_lower.clone());
                all_nodes.insert(pid_lower);
            }
        }
    }

    // 2. Topological sort (Kahn's or DFS). Using simple iteration since graph is small.
    let mut resolved: HashSet<String> = HashSet::new();
    let mut resolved_order: Vec<String> = Vec::new();
    let mut iteration = 0;

    while resolved.len() < all_nodes.len() {
        let mut made_progress = false;

        for node in &all_nodes {
            if resolved.contains(node) {
                continue;
            }

            // A node can be resolved if it has NO dependencies, OR its dependency is already resolved
            let can_resolve = match dependencies.get(node) {
                None => true,                              // No parent
                Some(parent) => resolved.contains(parent), // Parent is already resolved
            };

            if can_resolve {
                resolved.insert(node.clone());
                resolved_order.push(node.clone());
                made_progress = true;
            }
        }

        if !made_progress {
            warn!("Circular dependency or missing parent detected in skill inheritance");
            return Err(anyhow!(
                "Steering engine abort: Circular inheritance detected in extends fields"
            ));
        }

        iteration += 1;
        if iteration > 100 {
            return Err(anyhow!("Steering engine abort: Inheritance depth exceeded"));
        }
    }

    // 3. Apply inheritance in order
    for node_id in resolved_order {
        let parent_id = dependencies.get(&node_id).cloned();

        if let Some(pid) = parent_id {
            // we need to merge pid -> node_id

            // first grab a clone of the parent config
            let parent_cfg = skills.get(&pid).and_then(|s| s.config.clone());

            if let Some(p_cfg) = parent_cfg {
                if let Some(child_skill) = skills.get_mut(&node_id) {
                    if let Some(c_cfg) = &mut child_skill.config {
                        resolve_inheritance(c_cfg, &p_cfg)?;
                    }
                }
            } else {
                warn!(
                    "Skill {} extends {} which was not found or is missing config",
                    node_id, pid
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steering::types::*;

    fn create_test_skill(name: &str) -> SkillFile {
        SkillFile {
            meta: SkillMeta {
                id: name.to_string(),
                name: name.to_string(),
                version: "1.0".to_string(),
                description: format!("Test skill {}", name),
                author: "test".to_string(),
                tags: vec![],
                extends: None,
            },
            activation: SkillActivation::default(),
            directives: SkillDirectives::default(),
            routing: SkillRouting::default(),
            tools: SkillTools::default(),
            memory: SkillMemory::default(),
        }
    }

    #[test]
    fn test_resolve_inheritance_merges_tags() {
        let mut child = create_test_skill("child");
        let mut parent = create_test_skill("parent");

        parent.meta.tags = vec!["tag1".to_string(), "tag2".to_string()];
        child.meta.tags = vec!["tag3".to_string()];

        resolve_inheritance(&mut child, &parent).expect("inheritance should succeed");

        assert_eq!(child.meta.tags.len(), 3);
        assert!(child.meta.tags.contains(&"tag1".to_string()));
        assert!(child.meta.tags.contains(&"tag2".to_string()));
        assert!(child.meta.tags.contains(&"tag3".to_string()));
    }

    #[test]
    fn test_resolve_inheritance_child_overrides_parent() {
        let mut child = create_test_skill("child");
        let mut parent = create_test_skill("parent");

        parent.directives.system_prefix = "parent prefix".to_string();
        child.directives.system_prefix = "child prefix".to_string();

        resolve_inheritance(&mut child, &parent).expect("inheritance should succeed");

        // Child's system_prefix should be preserved with parent prepended
        assert!(child.directives.system_prefix.contains("parent prefix"));
        assert!(child.directives.system_prefix.contains("child prefix"));
    }

    #[test]
    fn test_resolve_inheritance_takes_parent_when_child_empty() {
        let mut child = create_test_skill("child");
        let mut parent = create_test_skill("parent");

        parent.directives.system_suffix = "parent suffix".to_string();
        child.directives.system_suffix = String::new();

        resolve_inheritance(&mut child, &parent).expect("inheritance should succeed");

        assert_eq!(child.directives.system_suffix, "parent suffix");
    }

    #[test]
    fn test_extend_unique_deduplicates() {
        let mut target = vec!["a".to_string(), "b".to_string()];
        let source = vec!["b".to_string(), "c".to_string()];

        extend_unique(&mut target, &source);

        assert_eq!(target.len(), 3);
        assert_eq!(
            target,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn test_build_inheritance_graph_detects_circular_dependency() {
        use crate::steering::loader::Skill;
        use std::path::PathBuf;

        let mut skills = HashMap::new();

        // Create skill A that extends B
        let mut skill_a = create_test_skill("a");
        skill_a.meta.extends = Some("b".to_string());
        skills.insert(
            "a".to_string(),
            Skill {
                name: "a".to_string(),
                description: "Skill A".to_string(),
                content: String::new(),
                file_path: PathBuf::from("a.toml"),
                config: Some(skill_a),
            },
        );

        // Create skill B that extends A (circular!)
        let mut skill_b = create_test_skill("b");
        skill_b.meta.extends = Some("a".to_string());
        skills.insert(
            "b".to_string(),
            Skill {
                name: "b".to_string(),
                description: "Skill B".to_string(),
                content: String::new(),
                file_path: PathBuf::from("b.toml"),
                config: Some(skill_b),
            },
        );

        let result = build_inheritance_graph(&mut skills);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Circular"));
    }
}
