use std::path::{Path, PathBuf};

use anyhow::Result;
use regex::Regex;
use sdk::core_tool::{CoreContext, CoreTool};
use sdk::errors::EngineError;
use sdk::tool_io::{ToolInput, ToolOutput};
use tokio::fs;
use tracing::{debug, info, warn};

#[derive(Debug)]
pub struct FilesystemTool {
    workspace: PathBuf,
    deny_list: Vec<PathBuf>,
}

impl FilesystemTool {
    pub fn new(workspace: PathBuf) -> Result<Self, EngineError> {
        let workspace = workspace.canonicalize().map_err(|e| {
            EngineError::Config(format!("Failed to canonicalize workspace path: {}", e))
        })?;
        Ok(Self {
            workspace,
            deny_list: default_deny_list(),
        })
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn validate_candidate_path(&self, path: &Path) -> Result<PathBuf, EngineError> {
        self.validate_path(path)
    }

    pub async fn read_file(&self, path: &str) -> Result<String> {
        let path = self.resolve_path(path)?;
        info!("Reading file: {}", path.display());
        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
        debug!("Read {} bytes from {}", content.len(), path.display());
        Ok(content)
    }

    pub async fn write_file(&self, path: &str, content: &str) -> Result<String> {
        let target = PathBuf::from(path);
        let validated = if target.exists() {
            self.resolve_path(path)?
        } else {
            let abs = if target.is_absolute() {
                target.clone()
            } else {
                self.workspace.join(&target)
            };
            self.check_denied(&abs).map_err(|e| {
                warn!("Path denied for new file {}: {}", abs.display(), e);
                anyhow::anyhow!("{}", e)
            })?;
            if let Some(parent) = abs.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).await.map_err(|e| {
                        anyhow::anyhow!("Failed to create directories {}: {}", parent.display(), e)
                    })?;
                }
                let canonical_parent = parent.canonicalize().map_err(|e| {
                    anyhow::anyhow!("Failed to resolve {}: {}", parent.display(), e)
                })?;
                if !canonical_parent.starts_with(&self.workspace) {
                    return Err(anyhow::anyhow!("Path outside workspace: {}", abs.display()));
                }
            }
            abs
        };

        info!(
            "Writing {} bytes to: {}",
            content.len(),
            validated.display()
        );
        fs::write(&validated, content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", validated.display(), e))?;
        Ok(format!(
            "Wrote {} bytes to {}",
            content.len(),
            validated.display()
        ))
    }

    /// Append `content` to the end of an existing file (or create it if absent).
    pub async fn append_to_file(&self, path: &str, content: &str) -> Result<String> {
        let target = PathBuf::from(path);
        let validated = if target.exists() {
            self.resolve_path(path)?
        } else {
            let abs = if target.is_absolute() {
                target.clone()
            } else {
                self.workspace.join(&target)
            };
            self.check_denied(&abs)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            if let Some(parent) = abs.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).await.map_err(|e| {
                        anyhow::anyhow!("Failed to create directories {}: {}", parent.display(), e)
                    })?;
                }
                let canonical_parent = parent.canonicalize().map_err(|e| {
                    anyhow::anyhow!("Failed to resolve {}: {}", parent.display(), e)
                })?;
                if !canonical_parent.starts_with(&self.workspace) {
                    return Err(anyhow::anyhow!("Path outside workspace: {}", abs.display()));
                }
            }
            abs
        };

        info!(
            "Appending {} bytes to: {}",
            content.len(),
            validated.display()
        );
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&validated)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", validated.display(), e))?;
        file.write_all(content.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to append to {}: {}", validated.display(), e))?;
        Ok(format!(
            "Appended {} bytes to {}",
            content.len(),
            validated.display()
        ))
    }

    /// Replace the first occurrence of `old` with `new` in the file at `path`.
    ///
    /// Returns an error when `old` is not found, so the agent knows the edit
    /// did not apply rather than silently succeeding.
    pub async fn patch_file(&self, path: &str, old: &str, new: &str) -> Result<String> {
        let resolved = self.resolve_path(path)?;
        let original = fs::read_to_string(&resolved)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", resolved.display(), e))?;

        if old.is_empty() {
            return Err(anyhow::anyhow!("patch_file: old_string must not be empty"));
        }

        let count = original.matches(old).count();
        if count == 0 {
            return Err(anyhow::anyhow!(
                "patch_file: old_string not found in {}",
                resolved.display()
            ));
        }
        if count > 1 {
            return Err(anyhow::anyhow!(
                "patch_file: old_string matches {} times in {} — make it more specific",
                count,
                resolved.display()
            ));
        }

        let patched = original.replacen(old, new, 1);
        fs::write(&resolved, &patched)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", resolved.display(), e))?;
        let added = new.lines().count().saturating_sub(old.lines().count());
        let removed = old.lines().count().saturating_sub(new.lines().count());
        let summary = match (added, removed) {
            (0, 0) => "patch applied (same line count)".to_string(),
            (a, 0) => format!("patch applied (+{} lines)", a),
            (0, r) => format!("patch applied (-{} lines)", r),
            (a, r) => format!("patch applied (+{} / -{} lines)", a, r),
        };
        info!("{} in {}", summary, resolved.display());
        Ok(summary)
    }

    // ----- sync variants used by the CoreTool handle() path -----------------

    fn append_to_file_sync(&self, path: &str, content: &str) -> Result<String> {
        use std::io::Write;
        let target = PathBuf::from(path);
        let validated = self.prepare_write_target(path, |parent| {
            std::fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!("Failed to create directories {}: {}", parent.display(), e)
            })
        })?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&validated)
            .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", validated.display(), e))?;
        file.write_all(content.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to append to {}: {}", validated.display(), e))?;
        let _ = target; // silence unused warning
        Ok(format!(
            "Appended {} bytes to {}",
            content.len(),
            validated.display()
        ))
    }

    fn patch_file_sync(&self, path: &str, old: &str, new: &str) -> Result<String> {
        let resolved = self.resolve_path(path)?;
        let original = std::fs::read_to_string(&resolved)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", resolved.display(), e))?;

        if old.is_empty() {
            return Err(anyhow::anyhow!("patch_file: old_string must not be empty"));
        }

        let count = original.matches(old).count();
        if count == 0 {
            return Err(anyhow::anyhow!(
                "patch_file: old_string not found in {}",
                resolved.display()
            ));
        }
        if count > 1 {
            return Err(anyhow::anyhow!(
                "patch_file: old_string matches {} times — make it more specific",
                count
            ));
        }

        let patched = original.replacen(old, new, 1);
        std::fs::write(&resolved, &patched)
            .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", resolved.display(), e))?;
        let added = new.lines().count().saturating_sub(old.lines().count());
        let removed = old.lines().count().saturating_sub(new.lines().count());
        let summary = match (added, removed) {
            (0, 0) => "patch applied (same line count)".to_string(),
            (a, 0) => format!("patch applied (+{} lines)", a),
            (0, r) => format!("patch applied (-{} lines)", r),
            (a, r) => format!("patch applied (+{} / -{} lines)", a, r),
        };
        Ok(summary)
    }

    pub async fn list_dir(&self, path: &str) -> Result<String> {
        let path = self.resolve_path(path)?;
        info!("Listing directory: {}", path.display());

        let mut entries = fs::read_dir(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read directory {}: {}", path.display(), e))?;
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        let mut links = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            let ft = entry.file_type().await?;
            if ft.is_dir() {
                dirs.push(format!("d  {}/", name));
            } else if ft.is_symlink() {
                links.push(format!("l  {}", name));
            } else {
                let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
                files.push(format!("f  {:>8}  {}", format_size(size), name));
            }
        }

        dirs.sort();
        files.sort();
        links.sort();

        let mut out = Vec::with_capacity(dirs.len() + files.len() + links.len() + 1);
        out.push(format!(
            "{}/  ({} entries)",
            path.display(),
            dirs.len() + files.len() + links.len()
        ));
        out.extend(dirs);
        out.extend(files);
        out.extend(links);
        Ok(out.join("\n"))
    }

    pub async fn file_exists(&self, path: &str) -> Result<bool> {
        match self.resolve_path(path) {
            Ok(p) => Ok(p.exists()),
            Err(_) => Ok(false),
        }
    }

    pub async fn delete_file(&self, path: &str) -> Result<String> {
        let path = self.resolve_path(path)?;
        info!("Deleting file: {}", path.display());
        let metadata = fs::metadata(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to inspect {}: {}", path.display(), e))?;
        if metadata.is_dir() {
            return Err(anyhow::anyhow!(
                "Refusing to delete directory {}; delete_file only removes files",
                path.display()
            ));
        }
        fs::remove_file(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete {}: {}", path.display(), e))?;
        Ok(format!("Deleted {}", path.display()))
    }

    pub async fn glob_files(
        &self,
        pattern: &str,
        path: Option<&str>,
        max_results: usize,
    ) -> Result<String> {
        self.glob_files_sync(pattern, path, max_results)
    }

    pub async fn grep_files(
        &self,
        pattern: &str,
        path: Option<&str>,
        file_pattern: Option<&str>,
        max_results: usize,
    ) -> Result<String> {
        self.grep_files_sync(pattern, path, file_pattern, max_results)
    }

    fn read_file_sync(&self, path: &str) -> Result<String> {
        let path = self.resolve_path(path)?;
        info!("Reading file: {}", path.display());
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
        debug!("Read {} bytes from {}", content.len(), path.display());
        Ok(content)
    }

    fn write_file_sync(&self, path: &str, content: &str) -> Result<String> {
        let validated = self.prepare_write_target(path, |parent| {
            std::fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!("Failed to create directories {}: {}", parent.display(), e)
            })
        })?;

        info!(
            "Writing {} bytes to: {}",
            content.len(),
            validated.display()
        );
        std::fs::write(&validated, content)
            .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", validated.display(), e))?;
        Ok(format!(
            "Wrote {} bytes to {}",
            content.len(),
            validated.display()
        ))
    }

    fn list_dir_sync(&self, path: &str) -> Result<String> {
        let path = self.resolve_path(path)?;
        info!("Listing directory: {}", path.display());

        let entries = std::fs::read_dir(&path)
            .map_err(|e| anyhow::anyhow!("Failed to read directory {}: {}", path.display(), e))?;
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        let mut links = Vec::new();

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let ft = entry.file_type()?;
            if ft.is_dir() {
                dirs.push(format!("d  {}/", name));
            } else if ft.is_symlink() {
                links.push(format!("l  {}", name));
            } else {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push(format!("f  {:>8}  {}", format_size(size), name));
            }
        }

        dirs.sort();
        files.sort();
        links.sort();

        let mut out = Vec::with_capacity(dirs.len() + files.len() + links.len() + 1);
        out.push(format!(
            "{}/  ({} entries)",
            path.display(),
            dirs.len() + files.len() + links.len()
        ));
        out.extend(dirs);
        out.extend(files);
        out.extend(links);
        Ok(out.join("\n"))
    }

    fn file_exists_sync(&self, path: &str) -> Result<bool> {
        match self.resolve_path(path) {
            Ok(p) => Ok(p.exists()),
            Err(_) => Ok(false),
        }
    }

    fn delete_file_sync(&self, path: &str) -> Result<String> {
        let path = self.resolve_path(path)?;
        info!("Deleting file: {}", path.display());
        let metadata = std::fs::metadata(&path)
            .map_err(|e| anyhow::anyhow!("Failed to inspect {}: {}", path.display(), e))?;
        if metadata.is_dir() {
            return Err(anyhow::anyhow!(
                "Refusing to delete directory {}; delete_file only removes files",
                path.display()
            ));
        }
        std::fs::remove_file(&path)
            .map_err(|e| anyhow::anyhow!("Failed to delete {}: {}", path.display(), e))?;
        Ok(format!("Deleted {}", path.display()))
    }

    fn glob_files_sync(
        &self,
        pattern: &str,
        path: Option<&str>,
        max_results: usize,
    ) -> Result<String> {
        if pattern.trim().is_empty() {
            return Err(anyhow::anyhow!("glob_files: pattern must not be empty"));
        }

        let search_root = self.resolve_path(path.unwrap_or("."))?;
        let matcher = compile_glob_regex(pattern)?;
        let mut results = Vec::new();
        let mut truncated = false;
        self.visit_paths(
            &search_root,
            &mut |candidate| {
                if candidate.is_file() {
                    let relative = workspace_relative(&self.workspace, candidate)?;
                    if matcher.is_match(&relative) {
                        results.push(relative);
                        if results.len() >= max_results {
                            truncated = true;
                            return Ok(VisitControl::Stop);
                        }
                    }
                }
                Ok(VisitControl::Continue)
            },
            true,
        )?;

        if results.is_empty() {
            return Ok(format!(
                "No files matched pattern '{}' under {}",
                pattern,
                search_root.display()
            ));
        }

        let mut lines = results;
        if truncated {
            lines.push(format!("(truncated to {} results)", max_results));
        }
        Ok(lines.join("\n"))
    }

    fn grep_files_sync(
        &self,
        pattern: &str,
        path: Option<&str>,
        file_pattern: Option<&str>,
        max_results: usize,
    ) -> Result<String> {
        if pattern.trim().is_empty() {
            return Err(anyhow::anyhow!("grep_files: pattern must not be empty"));
        }

        let search_root = self.resolve_path(path.unwrap_or("."))?;
        let content_regex = Regex::new(pattern)
            .map_err(|error| anyhow::anyhow!("grep_files: invalid regex: {}", error))?;
        let file_matcher = match file_pattern {
            Some(value) if !value.trim().is_empty() => Some(compile_glob_regex(value)?),
            _ => None,
        };

        let mut results = Vec::new();
        let mut truncated = false;
        self.visit_paths(
            &search_root,
            &mut |candidate| {
                if !candidate.is_file() {
                    return Ok(VisitControl::Continue);
                }

                let relative = workspace_relative(&self.workspace, candidate)?;
                if let Some(file_matcher) = &file_matcher {
                    if !file_matcher.is_match(&relative) {
                        return Ok(VisitControl::Continue);
                    }
                }

                let metadata = std::fs::metadata(candidate).map_err(|error| {
                    anyhow::anyhow!("Failed to inspect {}: {}", candidate.display(), error)
                })?;
                if metadata.len() > 2 * 1024 * 1024 {
                    return Ok(VisitControl::Continue);
                }

                let content = match std::fs::read_to_string(candidate) {
                    Ok(content) => content,
                    Err(_) => return Ok(VisitControl::Continue),
                };

                for (index, line) in content.lines().enumerate() {
                    if content_regex.is_match(line) {
                        results.push(format!(
                            "{}:{}:{}",
                            relative,
                            index + 1,
                            summarize_search_line(line)
                        ));
                        if results.len() >= max_results {
                            truncated = true;
                            return Ok(VisitControl::Stop);
                        }
                    }
                }

                Ok(VisitControl::Continue)
            },
            true,
        )?;

        if results.is_empty() {
            return Ok(format!(
                "No matches for regex '{}' under {}",
                pattern,
                search_root.display()
            ));
        }

        if truncated {
            results.push(format!("(truncated to {} results)", max_results));
        }
        Ok(results.join("\n"))
    }

    fn prepare_write_target<CreateDir>(&self, path: &str, create_dir: CreateDir) -> Result<PathBuf>
    where
        CreateDir: FnOnce(&Path) -> Result<()>,
    {
        let target = PathBuf::from(path);
        if target.exists() {
            return self.resolve_path(path);
        }

        let abs = if target.is_absolute() {
            target.clone()
        } else {
            self.workspace.join(&target)
        };
        self.check_denied(&abs).map_err(|e| {
            warn!("Path denied for new file {}: {}", abs.display(), e);
            anyhow::anyhow!("{}", e)
        })?;
        if let Some(parent) = abs.parent() {
            if !parent.exists() {
                create_dir(parent)?;
            }
            let canonical_parent = parent
                .canonicalize()
                .map_err(|e| anyhow::anyhow!("Failed to resolve {}: {}", parent.display(), e))?;
            if !canonical_parent.starts_with(&self.workspace) {
                return Err(anyhow::anyhow!("Path outside workspace: {}", abs.display()));
            }
        }

        Ok(abs)
    }

    fn visit_paths<F>(&self, root: &Path, visitor: &mut F, include_root: bool) -> Result<()>
    where
        F: FnMut(&Path) -> Result<VisitControl>,
    {
        if include_root {
            match visitor(root)? {
                VisitControl::Continue => {}
                VisitControl::Stop => return Ok(()),
            }
        }

        if root.is_file() {
            return Ok(());
        }

        let entries = std::fs::read_dir(root).map_err(|error| {
            anyhow::anyhow!("Failed to read directory {}: {}", root.display(), error)
        })?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            match visitor(&path)? {
                VisitControl::Continue => {}
                VisitControl::Stop => return Ok(()),
            }
            if file_type.is_dir() {
                self.visit_paths(&path, visitor, false)?;
            }
        }

        Ok(())
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        let expanded = expand_user_path(path);
        let target = Path::new(&expanded);
        let abs = if target.is_absolute() {
            target.to_path_buf()
        } else {
            self.workspace.join(target)
        };

        self.validate_path(&abs).map_err(|e| {
            warn!("Path validation failed for {}: {}", abs.display(), e);
            anyhow::anyhow!("{}", e)
        })
    }

    fn validate_path(&self, path: &Path) -> Result<PathBuf, EngineError> {
        let path_str = path.to_str().ok_or_else(|| {
            EngineError::PathCanonicalization(
                path.to_path_buf(),
                "Invalid UTF-8 in path".to_string(),
            )
        })?;

        if path_str.contains('\0')
            || path_str.contains("%2e")
            || path_str.contains("%2f")
            || path_str.contains("%5c")
            || path_str.contains("..\\")
        {
            return Err(EngineError::PathDenied(path.to_path_buf()));
        }

        let resolved = if path.is_relative() {
            self.workspace.join(path)
        } else {
            path.to_path_buf()
        };

        if self.is_denied(&resolved) {
            return Err(EngineError::PathDenied(resolved));
        }

        let canonical = if resolved.exists() {
            let first = resolved
                .canonicalize()
                .map_err(|e| EngineError::PathCanonicalization(resolved.clone(), e.to_string()))?;
            first
                .canonicalize()
                .map_err(|e| EngineError::PathCanonicalization(resolved.clone(), e.to_string()))?
        } else {
            let parent = resolved.parent().unwrap_or(Path::new("."));
            let filename = resolved.file_name().ok_or_else(|| {
                EngineError::PathCanonicalization(resolved.clone(), "No filename".to_string())
            })?;
            let canonical_parent = if parent.exists() {
                let first = parent.canonicalize().map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?;
                first.canonicalize().map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?
            } else {
                std::fs::create_dir_all(parent).map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?;
                let first = parent.canonicalize().map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?;
                first.canonicalize().map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?
            };
            canonical_parent.join(filename)
        };

        if self.is_denied(&canonical) {
            return Err(EngineError::PathDenied(canonical));
        }

        if !canonical.starts_with(&self.workspace) {
            return Err(EngineError::PathOutsideWorkspace(canonical));
        }

        Ok(canonical)
    }

    fn check_denied(&self, path: &Path) -> Result<(), EngineError> {
        if self.is_denied(path) {
            return Err(EngineError::PathDenied(path.to_path_buf()));
        }
        Ok(())
    }

    fn is_denied(&self, path: &Path) -> bool {
        self.deny_list.iter().any(|denied| {
            path.ends_with(denied)
                || path.components().any(|c| {
                    if let Some(os_str) = c.as_os_str().to_str() {
                        denied.as_os_str().to_str().is_some_and(|d| os_str == d)
                    } else {
                        false
                    }
                })
        })
    }
}

impl CoreTool for FilesystemTool {
    fn name(&self) -> &str {
        "filesystem"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn start(&mut self, _ctx: CoreContext) -> Result<(), EngineError> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        Ok(())
    }

    fn handle(&self, input: ToolInput) -> Result<ToolOutput, EngineError> {
        match input.method.as_str() {
            "read_file" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let value = self
                    .read_file_sync(&path)
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "write_file" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let content = input.param_str("content").map_err(tool_input_error)?;
                let value = self
                    .write_file_sync(&path, &content)
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "delete_file" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let value = self
                    .delete_file_sync(&path)
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "list_dir" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let value = self
                    .list_dir_sync(&path)
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "file_exists" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let value = self
                    .file_exists_sync(&path)
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "append_to_file" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let content = input.param_str("content").map_err(tool_input_error)?;
                let value = self
                    .append_to_file_sync(&path, &content)
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "patch_file" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let old = input.param_str("old_string").map_err(tool_input_error)?;
                let new = input.param_str("new_string").map_err(tool_input_error)?;
                let value = self
                    .patch_file_sync(&path, &old, &new)
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "glob_files" => {
                let pattern = input.param_str("pattern").map_err(tool_input_error)?;
                let path = input
                    .params
                    .get("path")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                let max_results = input
                    .params
                    .get("max_results")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(200) as usize;
                let value = self
                    .glob_files_sync(&pattern, path.as_deref(), max_results)
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "grep_files" => {
                let pattern = input.param_str("pattern").map_err(tool_input_error)?;
                let path = input
                    .params
                    .get("path")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                let file_pattern = input
                    .params
                    .get("file_pattern")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                let max_results = input
                    .params
                    .get("max_results")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(100) as usize;
                let value = self
                    .grep_files_sync(
                        &pattern,
                        path.as_deref(),
                        file_pattern.as_deref(),
                        max_results,
                    )
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            other => Err(EngineError::ToolError(format!(
                "Unknown filesystem method '{}'",
                other
            ))),
        }
    }
}

#[allow(improper_ctypes_definitions)]
#[no_mangle]
#[cfg(feature = "native-tool-entry")]
pub extern "C" fn create_tool() -> *mut dyn CoreTool {
    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let tool = FilesystemTool::new(workspace).expect("filesystem tool");
    Box::into_raw(Box::new(tool))
}

fn expand_user_path(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn default_deny_list() -> Vec<PathBuf> {
    vec![
        PathBuf::from(".ssh"),
        PathBuf::from("id_rsa"),
        PathBuf::from("id_ed25519"),
        PathBuf::from("id_dsa"),
        PathBuf::from("id_ecdsa"),
        PathBuf::from("id_ecdsa_sk"),
        PathBuf::from(".env"),
        PathBuf::from(".env.local"),
        PathBuf::from(".env.production"),
        PathBuf::from(".env.staging"),
        PathBuf::from(".netrc"),
        PathBuf::from(".git-credentials"),
        PathBuf::from(".aws/credentials"),
        PathBuf::from(".config/gcloud"),
        PathBuf::from(".azure"),
        PathBuf::from(".docker/config.json"),
        PathBuf::from(".kube/config"),
        PathBuf::from(".gnupg"),
        PathBuf::from(".npmrc"),
        PathBuf::from(".pypirc"),
        PathBuf::from(".yarnrc"),
        PathBuf::from(".cargo/credentials"),
        PathBuf::from(".config/gh/hosts.yml"),
        PathBuf::from("credentials"),
        PathBuf::from("private_key"),
    ]
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn tool_input_error(error: sdk::tool_io::ToolError) -> EngineError {
    EngineError::ToolError(error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitControl {
    Continue,
    Stop,
}

fn compile_glob_regex(pattern: &str) -> Result<Regex> {
    let mut regex = String::from("^");
    let chars: Vec<char> = pattern.replace('\\', "/").chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' => {
                if chars.get(index + 1) == Some(&'*') {
                    if chars.get(index + 2) == Some(&'/') {
                        regex.push_str("(?:.*/)?");
                        index += 3;
                    } else {
                        regex.push_str(".*");
                        index += 2;
                    }
                } else {
                    regex.push_str("[^/]*");
                    index += 1;
                }
            }
            '?' => {
                regex.push_str("[^/]");
                index += 1;
            }
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' => {
                regex.push('\\');
                regex.push(chars[index]);
                index += 1;
            }
            '/' => {
                regex.push('/');
                index += 1;
            }
            other => {
                regex.push(other);
                index += 1;
            }
        }
    }
    regex.push('$');
    Regex::new(&regex).map_err(|error| anyhow::anyhow!("Invalid glob '{}': {}", pattern, error))
}

fn workspace_relative(workspace: &Path, candidate: &Path) -> Result<String> {
    let relative = candidate
        .strip_prefix(workspace)
        .unwrap_or(candidate)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(relative)
}

fn summarize_search_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.len() > 180 {
        format!("{}...", &trimmed[..177])
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::FilesystemTool;
    use sdk::tool_io::ToolInput;
    use sdk::CoreTool;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn handle_write_file_does_not_require_tokio_runtime() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("rove-fs-tool-{}", unique));
        std::fs::create_dir_all(&workspace).expect("workspace");

        let tool = FilesystemTool::new(workspace.clone()).expect("tool");
        let output = tool
            .handle(
                ToolInput::new("write_file")
                    .with_param("path", serde_json::json!("temp.txt"))
                    .with_param("content", serde_json::json!("4")),
            )
            .expect("write file");

        assert!(output.success);
        assert_eq!(
            std::fs::read_to_string(workspace.join("temp.txt")).expect("read output file"),
            "4"
        );

        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn handle_glob_files_returns_matching_relative_paths() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("rove-fs-glob-{}", unique));
        std::fs::create_dir_all(workspace.join("src/nested")).expect("workspace");
        std::fs::write(workspace.join("src/lib.rs"), "pub fn lib() {}").expect("lib");
        std::fs::write(workspace.join("src/nested/mod.rs"), "pub fn nested() {}").expect("nested");

        let tool = FilesystemTool::new(workspace.clone()).expect("tool");
        let output = tool
            .handle(
                ToolInput::new("glob_files")
                    .with_param("pattern", serde_json::json!("src/**/*.rs")),
            )
            .expect("glob files");

        let value = output.data.as_str().expect("string output").to_string();
        assert!(value.contains("src/lib.rs"));
        assert!(value.contains("src/nested/mod.rs"));

        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn handle_grep_files_returns_line_matches() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("rove-fs-grep-{}", unique));
        std::fs::create_dir_all(workspace.join("src")).expect("workspace");
        std::fs::write(
            workspace.join("src/main.rs"),
            "fn alpha() {}\nfn beta_workflow() {}\n",
        )
        .expect("main");

        let tool = FilesystemTool::new(workspace.clone()).expect("tool");
        let output = tool
            .handle(
                ToolInput::new("grep_files")
                    .with_param("pattern", serde_json::json!("workflow"))
                    .with_param("file_pattern", serde_json::json!("src/**/*.rs")),
            )
            .expect("grep files");

        let value = output.data.as_str().expect("string output").to_string();
        assert!(value.contains("src/main.rs:2:fn beta_workflow() {}"));

        let _ = std::fs::remove_dir_all(&workspace);
    }
}
