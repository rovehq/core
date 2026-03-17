use sdk::errors::EngineError;
use std::path::{Path, PathBuf};

/// FileSystemGuard provides multi-layer defense against path traversal and unauthorized access.
///
/// It maintains a deny list of sensitive paths and performs double canonicalization checks
/// to prevent symlink-based bypass attacks.
///
/// # Platform-Specific Path Handling
///
/// This module uses Rust's `std::path::Path` and `PathBuf` types, which automatically
/// handle platform-specific path separators:
/// - Unix (Linux, macOS): forward slash (/)
/// - Windows: backslash (\)
///
/// The `canonicalize()` method resolves paths to their absolute form using the
/// platform-specific separator, ensuring consistent behavior across all platforms.
///
/// **Requirements**: 25.2 - Use platform-specific paths (/ on Unix, \ on Windows)
///
/// # Security Model
///
/// The guard implements a four-gate validation process:
/// 1. Check deny list before canonicalization
/// 2. Canonicalize path to resolve symlinks and .. patterns
/// 3. Check deny list after canonicalization
/// 4. Verify path is within workspace
#[derive(Debug)]
pub struct FileSystemGuard {
    workspace: PathBuf,
    deny_list: Vec<PathBuf>,
}

impl FileSystemGuard {
    /// Creates a new FileSystemGuard with the specified workspace directory.
    ///
    /// The deny list includes common sensitive paths:
    /// - .ssh (SSH keys)
    /// - .env (environment variables)
    /// - .aws/credentials (AWS credentials)
    /// - .config/gcloud (Google Cloud credentials)
    /// - id_rsa, id_ed25519 (SSH private keys)
    /// - .gnupg (GPG keys)
    /// - .kube/config (Kubernetes config)
    ///
    /// # Panics
    ///
    /// Panics if the workspace path cannot be canonicalized. This should only happen
    /// if the workspace doesn't exist or is inaccessible.
    pub fn new(workspace: PathBuf) -> Result<Self, sdk::errors::EngineError> {
        // Canonicalize workspace to handle symlinks (e.g., /var -> /private/var on macOS)
        let workspace = workspace.canonicalize().map_err(|e| {
            sdk::errors::EngineError::Config(format!(
                "Failed to canonicalize workspace path: {}",
                e
            ))
        })?;

        let deny_list = vec![
            // SSH keys and config
            PathBuf::from(".ssh"),
            PathBuf::from("id_rsa"),
            PathBuf::from("id_ed25519"),
            PathBuf::from("id_dsa"),
            PathBuf::from("id_ecdsa"),
            PathBuf::from("id_ecdsa_sk"),
            // Environment and dotfiles
            PathBuf::from(".env"),
            PathBuf::from(".env.local"),
            PathBuf::from(".env.production"),
            PathBuf::from(".env.staging"),
            PathBuf::from(".netrc"),
            PathBuf::from(".git-credentials"),
            // Cloud provider credentials
            PathBuf::from(".aws/credentials"),
            PathBuf::from(".config/gcloud"),
            PathBuf::from(".azure"),
            // Container and orchestration
            PathBuf::from(".docker/config.json"),
            PathBuf::from(".kube/config"),
            // GPG keys
            PathBuf::from(".gnupg"),
            // Package manager tokens
            PathBuf::from(".npmrc"),
            PathBuf::from(".pypirc"),
            PathBuf::from(".yarnrc"),
            PathBuf::from(".cargo/credentials"),
            // GitHub CLI
            PathBuf::from(".config/gh/hosts.yml"),
            // Generic sensitive names
            PathBuf::from("credentials"),
            PathBuf::from("private_key"),
        ];

        Ok(Self {
            workspace,
            deny_list,
        })
    }

    /// Validates a path through four security gates.
    ///
    /// # Security Gates
    ///
    /// 1. **Pre-canonicalization deny check**: Blocks obvious sensitive paths
    /// 2. **Canonicalization**: Resolves symlinks, .., and . components
    /// 3. **Post-canonicalization deny check**: Catches symlink-based bypasses
    /// 4. **Workspace boundary check**: Ensures path is within workspace
    ///
    /// # Errors
    ///
    /// Returns `EngineError::PathDenied` if the path matches the deny list.
    /// Returns `EngineError::PathCanonicalization` if canonicalization fails.
    /// Returns `EngineError::PathOutsideWorkspace` if the path is outside workspace.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::PathBuf;
    /// use rove_engine::fs_guard::FileSystemGuard;
    ///
    /// let guard = FileSystemGuard::new(PathBuf::from("/home/user/workspace"));
    ///
    /// // Valid path within workspace
    /// let valid = guard.validate_path(&PathBuf::from("/home/user/workspace/file.txt"));
    /// assert!(valid.is_ok());
    ///
    /// // Path traversal attempt
    /// let invalid = guard.validate_path(&PathBuf::from("/home/user/workspace/../.ssh/id_rsa"));
    /// assert!(invalid.is_err());
    /// ```
    pub fn validate_path(&self, path: &Path) -> Result<PathBuf, EngineError> {
        // Convert path to string for validation
        let path_str = path.to_str().ok_or_else(|| {
            EngineError::PathCanonicalization(path.to_path_buf(), "Invalid UTF-8 in path".to_string())
        })?;

        // Gate 0: Reject null bytes
        if path_str.contains('\0') {
            return Err(EngineError::PathDenied(path.to_path_buf()));
        }

        // Gate 0: Reject URL-encoded traversal
        if path_str.contains("%2e") || path_str.contains("%2f") || path_str.contains("%5c") {
            return Err(EngineError::PathDenied(path.to_path_buf()));
        }

        // Gate 0: Reject Windows-style traversal (even on non-Windows)
        if path_str.contains("..\\") {
            return Err(EngineError::PathDenied(path.to_path_buf()));
        }

        // Resolve relative paths against the workspace
        let resolved = if path.is_relative() {
            self.workspace.join(path)
        } else {
            path.to_path_buf()
        };

        // Gate 1: Check deny list before canonicalization
        if self.is_denied(&resolved) {
            return Err(EngineError::PathDenied(resolved));
        }

        // Gate 2: Double canonicalize (catches symlink attacks)
        let canonical = if resolved.exists() {
            let p1 = resolved
                .canonicalize()
                .map_err(|e| EngineError::PathCanonicalization(resolved.clone(), e.to_string()))?;
            p1.canonicalize()
                .map_err(|e| EngineError::PathCanonicalization(resolved.clone(), e.to_string()))?
        } else {
            // For new files, canonicalize the parent directory twice and join the filename
            let parent = resolved.parent().unwrap_or(Path::new("."));
            let filename = resolved.file_name().ok_or_else(|| {
                EngineError::PathCanonicalization(resolved.clone(), "No filename".to_string())
            })?;
            let canonical_parent = if parent.exists() {
                let p1 = parent.canonicalize().map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?;
                p1.canonicalize().map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?
            } else {
                // Create parent directories if they don't exist
                std::fs::create_dir_all(parent).map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?;
                let p1 = parent.canonicalize().map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?;
                p1.canonicalize().map_err(|e| {
                    EngineError::PathCanonicalization(resolved.clone(), e.to_string())
                })?
            };
            canonical_parent.join(filename)
        };

        // Gate 3: Check deny list after canonicalization (catches symlink bypasses)
        if self.is_denied(&canonical) {
            return Err(EngineError::PathDenied(canonical));
        }

        // Gate 4: Verify within workspace
        let canonical_workspace = self
            .workspace
            .canonicalize()
            .unwrap_or(self.workspace.clone());
        if !canonical.starts_with(&canonical_workspace) {
            return Err(EngineError::PathOutsideWorkspace(canonical));
        }

        Ok(canonical)
    }

    /// Checks if a path matches any entry in the deny list.
    ///
    /// This method checks both:
    /// - If the path ends with a denied component
    /// - If any component in the path matches a denied entry
    ///
    /// This catches patterns like:
    /// - `/home/user/.ssh/id_rsa` (ends with denied path)
    /// - `/tmp/.env` (contains denied component)
    /// - `workspace/../.ssh/id_rsa` (contains denied component)
    fn is_denied(&self, path: &Path) -> bool {
        self.deny_list.iter().any(|denied| {
            // Check if path ends with denied path
            path.ends_with(denied) ||
            // Check if any component matches denied entry
            path.components().any(|c| {
                if let Some(os_str) = c.as_os_str().to_str() {
                    denied.as_os_str().to_str().is_some_and(|d| os_str == d)
                } else {
                    false
                }
            })
        })
    }

    /// Returns a reference to the workspace path.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Check if a path is denied without requiring it to exist on disk.
    ///
    /// This is used to validate new file paths before creation, ensuring
    /// that files like `.env` cannot be created even if they don't exist yet.
    pub fn check_denied(&self, path: &Path) -> Result<(), EngineError> {
        if self.is_denied(path) {
            return Err(EngineError::PathDenied(path.to_path_buf()));
        }
        Ok(())
    }

    /// Returns a reference to the deny list.
    pub fn deny_list(&self) -> &[PathBuf] {
        &self.deny_list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_deny_list_before_canonicalization() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

        // Create a file with a denied name
        let denied_path = workspace.join(".ssh");
        let result = guard.validate_path(&denied_path);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
    }

    #[test]
    #[cfg(unix)]
    fn test_deny_list_after_canonicalization() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();

        // Create a symlink to a denied path
        let ssh_dir = temp.path().join(".ssh");
        fs::create_dir(&ssh_dir).unwrap();

        let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");
        let symlink_path = workspace.join("safe_link");
        std::os::unix::fs::symlink(&ssh_dir, &symlink_path).unwrap();

        let result = guard.validate_path(&symlink_path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
    }

    #[test]
    fn test_path_outside_workspace() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

        // Try to access parent directory
        let outside_path = temp.path().join("outside.txt");
        fs::write(&outside_path, "test").unwrap();

        let result = guard.validate_path(&outside_path);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::PathOutsideWorkspace(_)
        ));
    }

    #[test]
    fn test_valid_path_within_workspace() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

        // Create a valid file
        let valid_path = workspace.join("file.txt");
        fs::write(&valid_path, "test").unwrap();

        let result = guard.validate_path(&valid_path);
        if let Err(ref e) = result {
            eprintln!("Error: {:?}", e);
        }
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), valid_path.canonicalize().unwrap());
    }

    #[test]
    fn test_path_traversal_attempt() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

        // Create a file outside workspace
        let outside_file = temp.path().join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        // Try to access it via path traversal
        let traversal_path = workspace.join("..").join("secret.txt");
        let result = guard.validate_path(&traversal_path);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::PathOutsideWorkspace(_)
        ));
    }

    #[test]
    fn test_denied_component_in_path() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

        // Create a path with .env in the middle
        let env_path = workspace.join("project").join(".env").join("config");

        let result = guard.validate_path(&env_path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
    }

    #[test]
    fn test_rejects_null_byte() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

        // Try to validate a path with null byte
        let null_byte_path = workspace.join("file\0.txt");
        let result = guard.validate_path(&null_byte_path);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
    }

    #[test]
    fn test_rejects_url_encoded_traversal() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

        // Try URL-encoded path traversal patterns
        let encoded_patterns = vec![
            workspace.join("%2e%2e/etc/passwd"),
            workspace.join("..%2fetc%2fpasswd"),
            workspace.join("%5c%5c..%5c..%5cetc"),
        ];

        for pattern in encoded_patterns {
            let result = guard.validate_path(&pattern);
            assert!(result.is_err(), "Should reject URL-encoded traversal: {:?}", pattern);
            assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
        }
    }
}
