//! Filesystem Read/Write Core Tool
//!
//! Native filesystem operations for reading and writing files within the workspace.
//! All paths are validated through `FileSystemGuard` (4-gate security) before any I/O.

use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

use crate::fs_guard::FileSystemGuard;

#[derive(Debug)]
pub struct FilesystemTool {
    guard: FileSystemGuard,
}

impl FilesystemTool {
    pub fn new(workspace: PathBuf) -> Result<Self, sdk::errors::EngineError> {
        Ok(Self {
            guard: FileSystemGuard::new(workspace)?,
        })
    }

    /// Get the workspace path
    pub fn workspace(&self) -> &std::path::Path {
        self.guard.workspace()
    }

    /// Read the contents of a file within the workspace.
    pub async fn read_file(&self, path: &str) -> Result<String> {
        let path = self.resolve_path(path)?;
        info!("Reading file: {}", path.display());

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;

        debug!("Read {} bytes from {}", content.len(), path.display());
        Ok(content)
    }

    /// Write content to a file within the workspace.
    /// Creates parent directories if they don't exist.
    pub async fn write_file(&self, path: &str, content: &str) -> Result<String> {
        let target = PathBuf::from(path);

        // For new files that don't exist yet, validate the full target path
        // through FileSystemGuard to enforce deny list even for new files
        let validated = if target.exists() {
            self.resolve_path(path)?
        } else {
            // Build absolute path
            let abs = if target.is_absolute() {
                target.clone()
            } else {
                self.guard.workspace().join(&target)
            };

            // Check deny list on the target path BEFORE creating directories
            // This prevents creating files like workspace/.env that bypass the deny list
            self.guard.check_denied(&abs).map_err(|e| {
                warn!("Path denied for new file {}: {}", abs.display(), e);
                anyhow::anyhow!("{}", e)
            })?;

            if let Some(parent) = abs.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).await.map_err(|e| {
                        anyhow::anyhow!("Failed to create directories {}: {}", parent.display(), e)
                    })?;
                }
                // Validate the parent is within workspace
                let canonical_parent = parent.canonicalize().map_err(|e| {
                    anyhow::anyhow!("Failed to resolve {}: {}", parent.display(), e)
                })?;
                if !canonical_parent.starts_with(self.guard.workspace()) {
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

    /// List files and directories at the given path within the workspace.
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

    /// Check if a file or directory exists within the workspace.
    pub async fn file_exists(&self, path: &str) -> Result<bool> {
        match self.resolve_path(path) {
            Ok(p) => Ok(p.exists()),
            Err(_) => Ok(false),
        }
    }

    /// Resolve and validate a path through the FileSystemGuard.
    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        let target = Path::new(path);
        let abs = if target.is_absolute() {
            target.to_path_buf()
        } else {
            self.guard.workspace().join(target)
        };

        self.guard.validate_path(&abs).map_err(|e| {
            warn!("Path validation failed for {}: {}", abs.display(), e);
            anyhow::anyhow!("{}", e)
        })
    }
}

/// Format a byte count into a human-readable size string.
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, FilesystemTool) {
        let temp = TempDir::new().unwrap();
        let tool = FilesystemTool::new(temp.path().to_path_buf()).expect("test workspace");
        (temp, tool)
    }

    #[tokio::test]
    async fn test_write_and_read_file() {
        let (temp, tool) = setup();
        let file = temp.path().join("hello.txt");

        tool.write_file(file.to_str().unwrap(), "hello world")
            .await
            .unwrap();
        let content = tool.read_file(file.to_str().unwrap()).await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_write_creates_parent_dirs() {
        let (temp, tool) = setup();
        let file = temp.path().join("a/b/c/deep.txt");

        tool.write_file(file.to_str().unwrap(), "deep content")
            .await
            .unwrap();
        let content = tool.read_file(file.to_str().unwrap()).await.unwrap();
        assert_eq!(content, "deep content");
    }

    #[tokio::test]
    async fn test_read_nonexistent_file() {
        let (temp, tool) = setup();
        let file = temp.path().join("nope.txt");
        let result = tool.read_file(file.to_str().unwrap()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_dir() {
        let (temp, tool) = setup();
        std::fs::write(temp.path().join("a.txt"), "a").unwrap();
        std::fs::write(temp.path().join("b.txt"), "b").unwrap();
        std::fs::create_dir(temp.path().join("subdir")).unwrap();

        let listing = tool.list_dir(temp.path().to_str().unwrap()).await.unwrap();
        assert!(listing.contains("a.txt"));
        assert!(listing.contains("b.txt"));
        assert!(listing.contains("d  subdir/"));
        assert!(listing.contains("3 entries"));
    }

    #[tokio::test]
    async fn test_file_exists() {
        let (temp, tool) = setup();
        let file = temp.path().join("exists.txt");
        std::fs::write(&file, "hi").unwrap();

        assert!(tool.file_exists(file.to_str().unwrap()).await.unwrap());
        assert!(!tool
            .file_exists(temp.path().join("nope.txt").to_str().unwrap())
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_path_traversal_blocked() {
        let (temp, tool) = setup();
        // Create a file outside workspace
        let parent = temp.path().parent().unwrap();
        let outside = parent.join("outside_secret.txt");
        std::fs::write(&outside, "secret").unwrap();

        let traversal = temp.path().join("..").join("outside_secret.txt");
        let result = tool.read_file(traversal.to_str().unwrap()).await;
        assert!(result.is_err());

        // Clean up
        let _ = std::fs::remove_file(&outside);
    }

    #[tokio::test]
    async fn test_denied_path_blocked() {
        let (temp, tool) = setup();
        let ssh_dir = temp.path().join(".ssh");
        std::fs::create_dir(&ssh_dir).unwrap();
        std::fs::write(ssh_dir.join("id_rsa"), "private key").unwrap();

        let result = tool
            .read_file(ssh_dir.join("id_rsa").to_str().unwrap())
            .await;
        assert!(result.is_err());
    }
}
