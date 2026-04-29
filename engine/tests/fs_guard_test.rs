//! Tests for FileSystemGuard: validate_path(), check_denied(), deny_list()

use rove_engine::fs_guard::FileSystemGuard;
use sdk::errors::EngineError;
use std::fs;
use tempfile::TempDir;

fn make_guard(temp: &TempDir) -> FileSystemGuard {
    FileSystemGuard::new(temp.path().to_path_buf()).expect("create guard")
}

// ── Construction ──────────────────────────────────────────────────────────────

#[test]
fn guard_constructs_with_valid_workspace() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    assert!(guard.workspace().exists());
}

#[test]
fn guard_workspace_returns_path() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    assert!(guard.workspace().is_dir());
}

#[test]
fn guard_deny_list_is_nonempty() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    assert!(!guard.deny_list().is_empty());
}

#[test]
fn guard_nonexistent_workspace_returns_error() {
    let result = FileSystemGuard::new(std::path::PathBuf::from("/nonexistent/workspace/path"));
    assert!(result.is_err());
}

// ── Deny list coverage ────────────────────────────────────────────────────────

#[test]
fn deny_list_contains_ssh() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains_ssh = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy().contains(".ssh"));
    assert!(contains_ssh);
}

#[test]
fn deny_list_contains_env() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains_env = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy() == ".env");
    assert!(contains_env);
}

#[test]
fn deny_list_contains_aws_credentials() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains_aws = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy().contains(".aws"));
    assert!(contains_aws);
}

#[test]
fn deny_list_contains_gnupg() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains_gpg = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy().contains(".gnupg"));
    assert!(contains_gpg);
}

#[test]
fn deny_list_contains_kube_config() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains_kube = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy().contains(".kube"));
    assert!(contains_kube);
}

#[test]
fn deny_list_contains_id_rsa() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains_id_rsa = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy() == "id_rsa");
    assert!(contains_id_rsa);
}

#[test]
fn deny_list_contains_id_ed25519() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy() == "id_ed25519");
    assert!(contains);
}

#[test]
fn deny_list_contains_npmrc() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy().contains("npmrc"));
    assert!(contains);
}

#[test]
fn deny_list_contains_docker_config() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy().contains("docker"));
    assert!(contains);
}

#[test]
fn deny_list_contains_netrc() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let contains = guard
        .deny_list()
        .iter()
        .any(|p| p.to_string_lossy().contains("netrc"));
    assert!(contains);
}

// ── check_denied tests ────────────────────────────────────────────────────────

#[test]
fn check_denied_ssh_path_returns_error() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let ssh_path = std::path::Path::new(".ssh");
    assert!(guard.check_denied(ssh_path).is_err());
}

#[test]
fn check_denied_env_file_returns_error() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let env_path = std::path::Path::new(".env");
    assert!(guard.check_denied(env_path).is_err());
}

#[test]
fn check_denied_safe_path_returns_ok() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let safe_path = std::path::Path::new("safe_file.txt");
    assert!(guard.check_denied(safe_path).is_ok());
}

#[test]
fn check_denied_cargo_lock_is_ok() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let path = std::path::Path::new("Cargo.lock");
    assert!(guard.check_denied(path).is_ok());
}

#[test]
fn check_denied_src_main_is_ok() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let path = std::path::Path::new("src/main.rs");
    assert!(guard.check_denied(path).is_ok());
}

#[test]
fn check_denied_aws_credentials_returns_error() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let path = std::path::Path::new(".aws/credentials");
    assert!(guard.check_denied(path).is_err());
}

#[test]
fn check_denied_id_rsa_returns_error() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let path = std::path::Path::new("id_rsa");
    assert!(guard.check_denied(path).is_err());
}

#[test]
fn check_denied_gnupg_returns_error() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let path = std::path::Path::new(".gnupg");
    assert!(guard.check_denied(path).is_err());
}

#[test]
fn check_denied_returns_path_denied_error_variant() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let path = std::path::Path::new(".ssh");
    let err = guard.check_denied(path).unwrap_err();
    assert!(matches!(err, EngineError::PathDenied(_)));
}

// ── validate_path: valid paths ────────────────────────────────────────────────

#[test]
fn validate_path_existing_file_within_workspace_ok() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let file_path = temp.path().join("test_file.txt");
    fs::write(&file_path, "content").unwrap();
    let result = guard.validate_path(&file_path);
    assert!(result.is_ok());
}

#[test]
fn validate_path_returns_canonical_path() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let file_path = temp.path().join("canonicalize_test.txt");
    fs::write(&file_path, "content").unwrap();
    let result = guard.validate_path(&file_path).unwrap();
    assert!(result.is_absolute());
}

#[test]
fn validate_path_subdirectory_file_ok() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let subdir = temp.path().join("subdir");
    fs::create_dir(&subdir).unwrap();
    let file_path = subdir.join("file.txt");
    fs::write(&file_path, "content").unwrap();
    assert!(guard.validate_path(&file_path).is_ok());
}

#[test]
fn validate_path_new_file_in_workspace_ok() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    // File doesn't exist yet but will be in workspace
    let new_path = temp.path().join("new_file_to_create.txt");
    assert!(guard.validate_path(&new_path).is_ok());
}

#[test]
fn validate_path_deeply_nested_file_ok() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let nested = temp.path().join("a").join("b").join("c");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("deep.txt");
    fs::write(&file, "deep").unwrap();
    assert!(guard.validate_path(&file).is_ok());
}

// ── validate_path: denied paths ───────────────────────────────────────────────

#[test]
fn validate_path_ssh_dir_denied() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let ssh_path = temp.path().join(".ssh");
    let result = guard.validate_path(&ssh_path);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
}

#[test]
fn validate_path_env_file_denied() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let env_path = temp.path().join(".env");
    let result = guard.validate_path(&env_path);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
}

#[test]
fn validate_path_env_local_denied() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let path = temp.path().join(".env.local");
    let result = guard.validate_path(&path);
    assert!(result.is_err());
}

#[test]
fn validate_path_env_production_denied() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let path = temp.path().join(".env.production");
    let result = guard.validate_path(&path);
    assert!(result.is_err());
}

#[test]
fn validate_path_id_rsa_denied() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let path = temp.path().join("id_rsa");
    let result = guard.validate_path(&path);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
}

#[test]
fn validate_path_env_component_denied() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    // .env as a directory component in path
    let path = temp.path().join("project").join(".env").join("config");
    let result = guard.validate_path(&path);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
}

// ── validate_path: outside workspace ─────────────────────────────────────────

#[test]
fn validate_path_parent_directory_denied() {
    let temp = TempDir::new().unwrap();
    let subdir = temp.path().join("workspace");
    fs::create_dir(&subdir).unwrap();
    let guard = FileSystemGuard::new(subdir.clone()).expect("guard");
    let outside = temp.path().join("outside.txt");
    fs::write(&outside, "secret").unwrap();
    let result = guard.validate_path(&outside);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        EngineError::PathOutsideWorkspace(_)
    ));
}

#[test]
fn validate_path_traversal_blocked() {
    let temp = TempDir::new().unwrap();
    let subdir = temp.path().join("workspace");
    fs::create_dir(&subdir).unwrap();
    let guard = FileSystemGuard::new(subdir.clone()).expect("guard");
    let outside_file = temp.path().join("secret.txt");
    fs::write(&outside_file, "secret").unwrap();
    let traversal = subdir.join("..").join("secret.txt");
    let result = guard.validate_path(&traversal);
    assert!(result.is_err());
}

#[test]
fn validate_path_absolute_outside_workspace_denied() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    // /tmp itself should be outside workspace
    let outside = std::path::Path::new("/tmp");
    let result = guard.validate_path(outside);
    assert!(result.is_err());
}

// ── validate_path: null bytes ─────────────────────────────────────────────────

#[test]
fn validate_path_null_byte_rejected() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let null_path = temp.path().join("file\0.txt");
    let result = guard.validate_path(&null_path);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
}

// ── validate_path: URL-encoded patterns ───────────────────────────────────────

#[test]
fn validate_path_url_encoded_2e_rejected() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let encoded = temp.path().join("%2e%2e/etc/passwd");
    let result = guard.validate_path(&encoded);
    assert!(result.is_err());
}

#[test]
fn validate_path_url_encoded_2f_rejected() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let encoded = temp.path().join("..%2fetc%2fpasswd");
    let result = guard.validate_path(&encoded);
    assert!(result.is_err());
}

#[test]
fn validate_path_url_encoded_5c_rejected() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let encoded = temp.path().join("%5c%5c..%5c");
    let result = guard.validate_path(&encoded);
    assert!(result.is_err());
}

// ── Relative path resolution ──────────────────────────────────────────────────

#[test]
fn validate_path_relative_resolves_against_workspace() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    // A relative path with a safe name
    let file = temp.path().join("relative_test.txt");
    fs::write(&file, "content").unwrap();
    let rel = std::path::Path::new("relative_test.txt");
    let result = guard.validate_path(rel);
    assert!(result.is_ok());
}

// ── Symlink attacks (Unix only) ────────────────────────────────────────────────

#[test]
#[cfg(unix)]
fn validate_path_symlink_to_denied_blocked() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    // Create .ssh dir in temp root (not workspace)
    let ssh_dir = temp.path().join(".ssh");
    fs::create_dir(&ssh_dir).unwrap();
    // Symlink in workspace pointing to .ssh
    let link = temp.path().join("safe_link");
    std::os::unix::fs::symlink(&ssh_dir, &link).unwrap();
    let result = guard.validate_path(&link);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), EngineError::PathDenied(_)));
}

// ── Multiple file operations ──────────────────────────────────────────────────

#[test]
fn validate_path_multiple_files_all_ok() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let files = ["a.txt", "b.rs", "c.toml", "d.json", "e.yaml"];
    for name in &files {
        let path = temp.path().join(name);
        fs::write(&path, "content").unwrap();
        assert!(
            guard.validate_path(&path).is_ok(),
            "Expected ok for {}",
            name
        );
    }
}

#[test]
fn validate_path_many_denied_paths_all_fail() {
    let temp = TempDir::new().unwrap();
    let guard = make_guard(&temp);
    let denied = [".env", "id_rsa", ".gnupg", ".ssh"];
    for name in &denied {
        let path = temp.path().join(name);
        assert!(
            guard.validate_path(&path).is_err(),
            "Expected deny for {}",
            name
        );
    }
}
