//! Steering Engine & Hot-Reload Loader
//!
//! Loads and manages Agent Skills from TOML and Markdown files.
//! Integrates `notify` watcher for real-time hot-reloading.

use super::types::{MergedDirectives, RoutingPreferences, SkillFile};
use anyhow::{Context, Result};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Maximum priority allowed for user-created skills
const MAX_USER_PRIORITY: u16 = 90;

/// An Agent Skill loaded from a TOML or Markdown file
#[derive(Debug, Clone)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub file_path: PathBuf,
    /// Full TOML config (None for legacy .md skills)
    pub config: Option<SkillFile>,
}

/// The Steering Engine manages the library of available skills
pub struct SteeringEngine {
    skills_dirs: Vec<PathBuf>,
    skills: Arc<RwLock<HashMap<String, Skill>>>,
    active: Arc<RwLock<Vec<String>>>,
    // Channel for the watcher to notify the main engine tasks to perform a reload
    _watcher_tx: Option<mpsc::Sender<()>>,
}

impl SteeringEngine {
    /// Create a new Steering Engine and load skills, spawning a hot-reload watcher
    pub async fn new(skills_dir: &Path) -> Result<Self> {
        Self::new_with_workspace(skills_dir, Option::<&Path>::None).await
    }

    pub async fn new_with_workspace(
        global_dir: &Path,
        workspace_dir: Option<&Path>,
    ) -> Result<Self> {
        // Bootstrap built-in skills if they don't exist
        if let Err(e) = super::builtins::bootstrap_builtins(global_dir).await {
            tracing::warn!("Failed to bootstrap built-in skills: {}", e);
        }

        let skills = Arc::new(RwLock::new(HashMap::new()));
        let active = Arc::new(RwLock::new(Vec::new()));

        let mut skills_dirs = vec![global_dir.to_path_buf()];
        if let Some(workspace_dir) = workspace_dir {
            let workspace_dir = workspace_dir.to_path_buf();
            if workspace_dir != global_dir {
                skills_dirs.push(workspace_dir);
            }
        }

        for dir in &skills_dirs {
            if !dir.exists() || !dir.is_dir() {
                info!("Steering directory {} does not exist yet.", dir.display());
                fs::create_dir_all(dir).await.ok();
            }
        }

        let (tx, mut rx) = mpsc::channel(100);

        let mut engine = Self {
            skills_dirs: skills_dirs.clone(),
            skills: skills.clone(),
            active: active.clone(),
            _watcher_tx: Some(tx.clone()),
        };

        // Initial Load
        engine.load_all_skills().await?;

        // Spawn a background task to listen for file system events and trigger reloads
        let dirs_clone = skills_dirs.clone();
        let skills_clone = skills.clone();
        let active_clone = active.clone();

        tokio::spawn(async move {
            let (event_tx, mut event_rx) = mpsc::channel(100);

            // Set up notify watcher
            let watcher_res = RecommendedWatcher::new(
                move |res: notify::Result<Event>| {
                    if let Ok(event) = res {
                        // Filter for file creation, modification, or deletion
                        if event.kind.is_create()
                            || event.kind.is_modify()
                            || event.kind.is_remove()
                        {
                            let _ = event_tx.blocking_send(());
                        }
                    }
                },
                Config::default(),
            );

            let mut watcher = match watcher_res {
                Ok(w) => w,
                Err(e) => {
                    error!("Failed to initialize file watcher for steering: {}", e);
                    return;
                }
            };

            for dir in &dirs_clone {
                if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
                    error!(
                        "Failed to watch steering directory {}: {}",
                        dir.display(),
                        e
                    );
                } else {
                    info!("Watching for steering changes in {}", dir.display());
                }
            }

            // Debounce events to prevent thrashing
            loop {
                tokio::select! {
                    Some(_) = event_rx.recv() => {
                        // Wait a short bit to allow multiple fast file saves to settle
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        // Drain any other events that came in during the sleep
                        while event_rx.try_recv().is_ok() {}

                        info!("Detected changes in steering files. Reloading...");
                        // Perform the reload
                        if let Err(e) = Self::perform_reload(&dirs_clone, &skills_clone, &active_clone).await {
                            error!("Failed to hot-reload steering skills: {:?}", e);
                        } else {
                            // Let the engine know if we need to manually trigger via tx (reserved for manual hooks)
                             let _ = tx.send(()).await;
                        }
                    }
                    else => break, // Channel closed
                }
            }
        });

        // Drain initial event queue from setup
        while rx.try_recv().is_ok() {}

        Ok(engine)
    }

    /// Load all `.toml` and `.md` files in the skills directory (Internal)
    async fn perform_reload(
        dirs: &[PathBuf],
        skills_lock: &Arc<RwLock<HashMap<String, Skill>>>,
        active_lock: &Arc<RwLock<Vec<String>>>,
    ) -> Result<()> {
        let mut new_skills = HashMap::new();

        for dir in dirs {
            if !dir.exists() {
                continue;
            }

            let mut entries = fs::read_dir(dir)
                .await
                .with_context(|| format!("Failed to read steering directory {}", dir.display()))?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                let ext = path.extension().and_then(|s| s.to_str());
                let result = match ext {
                    Some("toml") => Self::parse_toml_skill(&path).await,
                    Some("md") => Self::parse_md_skill(&path).await,
                    _ => continue,
                };

                match result {
                    Ok(skill) => {
                        new_skills.insert(skill.id.clone(), skill);
                    }
                    Err(e) => {
                        warn!("Failed to parse steering file {}: {}", path.display(), e);
                    }
                }
            }
        }

        // Apply topological inheritance based on extends mapping
        if let Err(e) = super::resolver::build_inheritance_graph(&mut new_skills) {
            error!("Skill inheritance conflict detected during reload: {}", e);
            // We still keep the mapping even if some dependencies are broken,
            // to prevent complete engine failure.
        }

        // Swap the maps
        let mut s_write = skills_lock.write().await;
        *s_write = new_skills;

        // Re-validate active skills
        let mut a_write = active_lock.write().await;
        let previously_active = a_write.clone();
        a_write.clear();
        for id in previously_active {
            if s_write.contains_key(&id) {
                a_write.push(id);
            }
        }

        Ok(())
    }

    /// Primary manual load hook (used on startup)
    pub async fn load_all_skills(&mut self) -> Result<()> {
        Self::perform_reload(&self.skills_dirs, &self.skills, &self.active).await
    }

    /// Parse a TOML skill file
    async fn parse_toml_skill(path: &Path) -> Result<Skill> {
        let content = fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read {}", path.display()))?;

        let mut skill_file: SkillFile = toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML skill {}", path.display()))?;

        // Enforce max priority for non-built-in skills
        if skill_file.activation.priority > MAX_USER_PRIORITY {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let builtin = [
                "general",
                "code",
                "git",
                "shell",
                "security",
                "careful",
                "fast",
                "code-review",
                "deep-research",
                "local-only",
                "local_only",
                "sensitive",
            ];
            if !builtin.contains(&stem) {
                warn!(
                    "Skill {} has priority {} > {}, capping to {}",
                    skill_file.meta.id,
                    skill_file.activation.priority,
                    MAX_USER_PRIORITY,
                    MAX_USER_PRIORITY
                );
                skill_file.activation.priority = MAX_USER_PRIORITY;
            }
        }

        // Build system prompt content from directives
        let mut display_content = String::new();
        if !skill_file.directives.system_prefix.is_empty() {
            display_content.push_str(&skill_file.directives.system_prefix);
        }
        if !skill_file.directives.system_suffix.is_empty() {
            if !display_content.is_empty() {
                display_content.push_str("\n\n");
            }
            display_content.push_str(&skill_file.directives.system_suffix);
        }

        Ok(Skill {
            id: skill_file.meta.id.to_lowercase(),
            name: skill_file.meta.name.clone(),
            description: skill_file.meta.description.clone(),
            content: display_content,
            file_path: path.to_path_buf(),
            config: Some(skill_file),
        })
    }

    /// Parse a legacy Markdown file containing YAML frontmatter
    async fn parse_md_skill(path: &Path) -> Result<Skill> {
        let file_content = fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read {}", path.display()))?;

        if !file_content.starts_with("---\n") && !file_content.starts_with("---\r\n") {
            return Err(anyhow::anyhow!("Missing YAML frontmatter in skill"));
        }

        let parts: Vec<&str> = file_content.splitn(3, "---").collect();
        if parts.len() < 3 {
            return Err(anyhow::anyhow!(
                "Malformed YAML frontmatter (missing closing ---)"
            ));
        }

        let frontmatter_str = parts[1];
        let content_str = parts[2].trim().to_string();

        let mut name = None;
        let mut description = None;

        for line in frontmatter_str.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("name:") {
                name = Some(rest.trim().trim_matches('"').trim_matches('\'').to_string());
            } else if let Some(rest) = line.strip_prefix("description:") {
                description = Some(rest.trim().trim_matches('"').trim_matches('\'').to_string());
            }
        }

        let name = name.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        });
        let description = description.unwrap_or_else(|| "No description provided.".to_string());

        Ok(Skill {
            id: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_lowercase(),
            name,
            description,
            content: content_str,
            file_path: path.to_path_buf(),
            config: None,
        })
    }

    // --- Public API ---

    pub async fn activate(&self, skill_id: &str) -> Result<()> {
        let key = skill_id.to_lowercase();
        let skills = self.skills.read().await;

        if !skills.contains_key(&key) {
            return Err(anyhow::anyhow!("Skill '{}' not found", skill_id));
        }

        let mut active = self.active.write().await;

        if active.contains(&key) {
            return Ok(());
        }

        // Check conflicts
        if let Some(skill) = skills.get(&key) {
            if let Some(ref cfg) = skill.config {
                let mut to_deactivate = Vec::new();
                for conflict in &cfg.activation.conflicts_with {
                    let conflict_key = conflict.to_lowercase();
                    if active.contains(&conflict_key) {
                        let conflict_priority = skills
                            .get(&conflict_key)
                            .and_then(|s| s.config.as_ref())
                            .map(|c| c.activation.priority)
                            .unwrap_or(0);
                        let new_priority = cfg.activation.priority;

                        if new_priority >= conflict_priority {
                            info!(
                                "Skill '{}' deactivated: conflicts with '{}'",
                                conflict, skill_id
                            );
                            to_deactivate.push(conflict_key);
                        } else {
                            return Err(anyhow::anyhow!(
                                "Cannot activate '{}': conflicts with '{}' (higher priority)",
                                skill_id,
                                conflict
                            ));
                        }
                    }
                }
                active.retain(|a| !to_deactivate.contains(a));
            }
        }

        active.push(key);
        info!("Skill '{}' activated", skill_id);
        Ok(())
    }

    pub async fn deactivate(&self, skill_id: &str) {
        let key = skill_id.to_lowercase();
        let mut active = self.active.write().await;
        active.retain(|a| a != &key);
        info!("Skill '{}' deactivated", skill_id);
    }

    pub async fn auto_activate(&self, task_input: &str, risk_tier: u8, domain: Option<&str>) {
        let input_lower = task_input.to_lowercase();
        let domain_lower = domain.map(|value| value.to_ascii_lowercase());
        let skills = self.skills.read().await;
        let mut active = self.active.write().await;

        for (key, skill) in skills.iter() {
            if active.contains(key) {
                continue;
            }
            let cfg = match &skill.config {
                Some(c) => c,
                None => continue,
            };

            let domain_activates = domain_lower.as_ref().is_some_and(|domain| {
                cfg.meta
                    .domains
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(domain))
            });

            for pattern in &cfg.activation.auto_when {
                let should_activate = if let Some(rest) = pattern.strip_prefix("task contains:") {
                    let keywords: Vec<&str> = rest.trim().split('|').map(|s| s.trim()).collect();
                    keywords.iter().any(|kw| input_lower.contains(kw))
                } else if let Some(rest) = pattern.strip_prefix("file type:") {
                    let extensions: Vec<&str> = rest.trim().split('|').map(|s| s.trim()).collect();
                    extensions.iter().any(|ext| input_lower.contains(ext))
                } else {
                    false
                };

                let risk_activates = cfg
                    .activation
                    .auto_when_risk_tier
                    .map(|t| risk_tier >= t)
                    .unwrap_or(false);

                if domain_activates || should_activate || risk_activates {
                    debug!("Auto-activating skill '{}'", skill.name);
                    let key_clone = key.clone();
                    if !active.contains(&key_clone) {
                        active.push(key_clone);
                    }
                    break;
                }
            }

            if domain_activates && !active.contains(key) {
                debug!("Auto-activating steering file '{}' for domain", skill.name);
                active.push(key.clone());
            }
        }

        // Resolving conflicts within the lock without borrowing self again
        let mut to_deactivate = Vec::new();
        for active_key in active.iter() {
            if let Some(skill) = skills.get(active_key) {
                if let Some(ref cfg) = skill.config {
                    for conflict in &cfg.activation.conflicts_with {
                        let conflict_key = conflict.to_lowercase();
                        if active.contains(&conflict_key) && !to_deactivate.contains(&conflict_key)
                        {
                            let my_priority = cfg.activation.priority;
                            let their_priority = skills
                                .get(&conflict_key)
                                .and_then(|s| s.config.as_ref())
                                .map(|c| c.activation.priority)
                                .unwrap_or(0);

                            if my_priority >= their_priority {
                                to_deactivate.push(conflict_key.clone());
                            } else {
                                to_deactivate.push(active_key.clone());
                            }
                        }
                    }
                }
            }
        }

        for key in to_deactivate {
            active.retain(|a| a != &key);
        }
    }

    pub async fn get_directives(&self) -> MergedDirectives {
        self.get_directives_for_task("").await
    }

    pub async fn get_directives_for_task(&self, task_input: &str) -> MergedDirectives {
        let mut directives = MergedDirectives::default();
        let active = self.active.read().await;
        let skills = self.skills.read().await;
        let task_input_lower = task_input.to_lowercase();

        let mut active_skills: Vec<&Skill> =
            active.iter().filter_map(|key| skills.get(key)).collect();

        active_skills.sort_by(|a, b| {
            let pa = a
                .config
                .as_ref()
                .map(|c| c.activation.priority)
                .unwrap_or(0);
            let pb = b
                .config
                .as_ref()
                .map(|c| c.activation.priority)
                .unwrap_or(0);
            pb.cmp(&pa)
        });

        for skill in &active_skills {
            if let Some(ref cfg) = skill.config {
                if !cfg.directives.system_prefix.is_empty() {
                    if !directives.system_prefix.is_empty() {
                        directives.system_prefix.push('\n');
                    }
                    directives
                        .system_prefix
                        .push_str(&cfg.directives.system_prefix);
                }

                if !cfg.directives.system_suffix.is_empty() {
                    if !directives.system_suffix.is_empty() {
                        directives.system_suffix.push('\n');
                    }
                    directives
                        .system_suffix
                        .push_str(&cfg.directives.system_suffix);
                }

                for (stage, directive) in &cfg.directives.per_stage {
                    directives
                        .per_stage
                        .entry(stage.clone())
                        .or_insert_with(|| directive.clone());
                }

                for (pattern, hint) in &cfg.hints {
                    if !task_input_lower.is_empty()
                        && task_input_lower.contains(&pattern.to_ascii_lowercase())
                    {
                        if !directives.system_suffix.is_empty() {
                            directives.system_suffix.push('\n');
                        }
                        directives.system_suffix.push_str(hint);
                    }
                }

                directives.auto_tags.extend(cfg.memory.auto_tag.clone());
            } else if !skill.content.is_empty() {
                if !directives.system_prefix.is_empty() {
                    directives.system_prefix.push('\n');
                }
                directives
                    .system_prefix
                    .push_str(&format!("# {}\n{}", skill.name, skill.content));
            }
        }

        directives
    }

    pub async fn get_routing_prefs(&self) -> RoutingPreferences {
        let mut prefs = RoutingPreferences {
            min_score_threshold: 0.65,
            ..Default::default()
        };

        let active = self.active.read().await;
        let skills = self.skills.read().await;

        let mut active_skills: Vec<&Skill> =
            active.iter().filter_map(|key| skills.get(key)).collect();

        active_skills.sort_by(|a, b| {
            let pa = a
                .config
                .as_ref()
                .map(|c| c.activation.priority)
                .unwrap_or(0);
            let pb = b
                .config
                .as_ref()
                .map(|c| c.activation.priority)
                .unwrap_or(0);
            pb.cmp(&pa)
        });

        let mut got_providers = false;
        let mut got_mode = false;

        for skill in &active_skills {
            if let Some(ref cfg) = skill.config {
                if !got_providers && !cfg.routing.preferred_providers.is_empty() {
                    prefs.preferred_providers = cfg.routing.preferred_providers.clone();
                    got_providers = true;
                }

                for p in &cfg.routing.avoid_providers {
                    if !prefs.avoid_providers.contains(p) {
                        prefs.avoid_providers.push(p.clone());
                    }
                }

                if !got_mode {
                    if let Some(ref mode) = cfg.routing.prefer_mode {
                        prefs.prefer_mode = Some(mode.clone());
                        got_mode = true;
                    }
                }

                if cfg.routing.always_verify {
                    prefs.always_verify = true;
                }

                if let Some(threshold) = cfg.routing.min_score_threshold {
                    if threshold > prefs.min_score_threshold {
                        prefs.min_score_threshold = threshold;
                    }
                }
            }
        }

        prefs
    }

    pub async fn matched_hints(&self, task_input: &str) -> Vec<String> {
        let task_input_lower = task_input.to_ascii_lowercase();
        let active = self.active.read().await;
        let skills = self.skills.read().await;
        let mut matched = Vec::new();

        for skill_id in active.iter() {
            let Some(skill) = skills.get(skill_id) else {
                continue;
            };
            let Some(cfg) = &skill.config else {
                continue;
            };

            for (pattern, hint) in &cfg.hints {
                if task_input_lower.contains(&pattern.to_ascii_lowercase()) {
                    matched.push(hint.clone());
                }
            }
        }

        matched
    }

    pub async fn get_skill(&self, name: &str) -> Option<Skill> {
        let skills = self.skills.read().await;
        skills.get(&name.to_lowercase()).cloned()
    }

    pub async fn list_skills(&self) -> Vec<Skill> {
        let skills = self.skills.read().await;
        skills.values().cloned().collect()
    }

    pub async fn active_skills(&self) -> Vec<String> {
        let active = self.active.read().await;
        active.clone()
    }

    pub async fn is_active(&self, skill_id: &str) -> bool {
        let active = self.active.read().await;
        active.contains(&skill_id.to_lowercase())
    }
}
