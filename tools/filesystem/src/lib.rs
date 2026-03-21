use std::path::{Path, PathBuf};

use anyhow::Result;
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
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|error| EngineError::ToolError(error.to_string()))?;

        match input.method.as_str() {
            "read_file" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let value = runtime
                    .block_on(self.read_file(&path))
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "write_file" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let content = input.param_str("content").map_err(tool_input_error)?;
                let value = runtime
                    .block_on(self.write_file(&path, &content))
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "delete_file" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let value = runtime
                    .block_on(self.delete_file(&path))
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "list_dir" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let value = runtime
                    .block_on(self.list_dir(&path))
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                Ok(ToolOutput::json(serde_json::json!(value)))
            }
            "file_exists" => {
                let path = input.param_str("path").map_err(tool_input_error)?;
                let value = runtime
                    .block_on(self.file_exists(&path))
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
