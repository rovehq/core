//! Extended tests for security::fs_guard — additional validate_path, check_denied edge cases

use rove_engine::security::fs_guard::FileSystemGuard;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn make_guard() -> (TempDir, FileSystemGuard) {
    let temp = TempDir::new().unwrap();
    let guard = FileSystemGuard::new(temp.path().to_path_buf()).unwrap();
    (temp, guard)
}

// ── Construction variants ─────────────────────────────────────────────────────

#[test]
fn guard_new_with_real_temp_dir() {
    let temp = TempDir::new().unwrap();
    assert!(FileSystemGuard::new(temp.path().to_path_buf()).is_ok());
}

#[test]
fn guard_deny_list_not_empty() {
    let (_t, g) = make_guard();
    assert!(!g.deny_list().is_empty());
}

// ── check_denied: all default patterns ────────────────────────────────────────

#[test]
fn ssh_dir_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new(".ssh")).is_err());
}

#[test]
fn env_file_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new(".env")).is_err());
}

#[test]
fn aws_dir_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new(".aws")).is_err());
}

#[test]
fn gnupg_dir_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new(".gnupg")).is_err());
}

#[test]
fn kube_dir_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new(".kube")).is_err());
}

#[test]
fn id_rsa_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new("id_rsa")).is_err());
}

// ── check_denied: patterns with paths ────────────────────────────────────────

#[test]
fn path_with_ssh_component_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new("/home/user/.ssh/id_rsa")).is_err());
}

#[test]
fn path_with_env_component_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new("/project/.env")).is_err());
}

#[test]
fn path_with_aws_component_denied() {
    let (_t, g) = make_guard();
    assert!(g
        .check_denied(Path::new("/home/user/.aws/credentials"))
        .is_err());
}

// ── check_denied: safe paths not denied ──────────────────────────────────────

#[test]
fn src_main_rs_not_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new("src/main.rs")).is_ok());
}

#[test]
fn workspace_not_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new("workspace")).is_ok());
}

#[test]
fn readme_not_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new("README.md")).is_ok());
}

#[test]
fn cargo_toml_not_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new("Cargo.toml")).is_ok());
}

#[test]
fn empty_path_not_denied() {
    let (_t, g) = make_guard();
    assert!(g.check_denied(Path::new("")).is_ok());
}

// ── validate_path: valid paths in workspace ───────────────────────────────────

#[test]
fn validate_file_in_workspace_ok() {
    let (temp, g) = make_guard();
    let file = temp.path().join("testfile.txt");
    fs::write(&file, "hello").unwrap();
    assert!(g.validate_path(&file).is_ok());
}

#[test]
fn validate_subdir_file_ok() {
    let (temp, g) = make_guard();
    let subdir = temp.path().join("subdir");
    fs::create_dir_all(&subdir).unwrap();
    let file = subdir.join("file.rs");
    fs::write(&file, "fn main(){}").unwrap();
    assert!(g.validate_path(&file).is_ok());
}

#[test]
fn validate_nested_subdir_ok() {
    let (temp, g) = make_guard();
    let nested = temp.path().join("a").join("b").join("c");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("deep.txt");
    fs::write(&file, "deep").unwrap();
    assert!(g.validate_path(&file).is_ok());
}

// ── validate_path: denied patterns ───────────────────────────────────────────

#[test]
fn validate_env_file_in_workspace_denied() {
    let (temp, g) = make_guard();
    let path = temp.path().join(".env");
    fs::write(&path, "SECRET=abc").unwrap();
    assert!(g.validate_path(&path).is_err());
}

#[test]
fn validate_ssh_dir_in_workspace_denied() {
    let (temp, g) = make_guard();
    let path = temp.path().join(".ssh");
    fs::create_dir_all(&path).unwrap();
    assert!(g.validate_path(&path).is_err());
}

// ── validate_path: path traversal ────────────────────────────────────────────

#[test]
fn validate_dotdot_outside_workspace_denied() {
    let (temp, g) = make_guard();
    let path = temp.path().join("..").join("outside.txt");
    let result = g.validate_path(&path);
    assert!(result.is_err());
}

#[test]
fn validate_absolute_outside_workspace_denied() {
    let (_temp, g) = make_guard();
    let result = g.validate_path(Path::new("/etc/passwd"));
    assert!(result.is_err());
}

#[test]
fn validate_root_denied() {
    let (_temp, g) = make_guard();
    let result = g.validate_path(Path::new("/"));
    assert!(result.is_err());
}

// ── validate_path: nonexistent paths ─────────────────────────────────────────

#[test]
fn validate_nonexistent_in_workspace_is_ok_or_err() {
    let (temp, g) = make_guard();
    let path = temp.path().join("nonexistent_file_xyz.txt");
    let _ = g.validate_path(&path);
}

#[test]
fn validate_nonexistent_outside_workspace_denied() {
    let (_temp, g) = make_guard();
    let result = g.validate_path(Path::new("/nonexistent/outside"));
    assert!(result.is_err());
}

// ── deny_list accessors ───────────────────────────────────────────────────────

#[test]
fn deny_list_contains_ssh() {
    let (_t, g) = make_guard();
    let list = g.deny_list();
    assert!(list.iter().any(|p| p.to_string_lossy().contains(".ssh")));
}

#[test]
fn deny_list_contains_env() {
    let (_t, g) = make_guard();
    let list = g.deny_list();
    assert!(list.iter().any(|p| p.to_string_lossy().contains(".env")));
}

#[test]
fn deny_list_contains_aws() {
    let (_t, g) = make_guard();
    let list = g.deny_list();
    assert!(list.iter().any(|p| p.to_string_lossy().contains(".aws")));
}

#[test]
fn deny_list_is_static_across_calls() {
    let (_t, g) = make_guard();
    let l1 = g.deny_list();
    let l2 = g.deny_list();
    assert_eq!(l1.len(), l2.len());
}

#[test]
fn deny_list_entries_not_empty() {
    let (_t, g) = make_guard();
    for entry in g.deny_list() {
        assert!(!entry.to_string_lossy().is_empty());
    }
}

// ── Multiple guard instances ──────────────────────────────────────────────────

#[test]
fn two_guards_same_workspace_independent() {
    let temp = TempDir::new().unwrap();
    let g1 = FileSystemGuard::new(temp.path().to_path_buf()).unwrap();
    let g2 = FileSystemGuard::new(temp.path().to_path_buf()).unwrap();
    assert_eq!(g1.deny_list().len(), g2.deny_list().len());
}

#[test]
fn guards_for_different_workspaces_both_work() {
    let t1 = TempDir::new().unwrap();
    let t2 = TempDir::new().unwrap();
    let g1 = FileSystemGuard::new(t1.path().to_path_buf()).unwrap();
    let g2 = FileSystemGuard::new(t2.path().to_path_buf()).unwrap();

    let f1 = t1.path().join("file.txt");
    let f2 = t2.path().join("file.txt");
    fs::write(&f1, "a").unwrap();
    fs::write(&f2, "b").unwrap();

    assert!(g1.validate_path(&f1).is_ok());
    assert!(g2.validate_path(&f2).is_ok());
    assert!(g1.validate_path(&f2).is_err());
}

// ── Absolute paths outside workspace ─────────────────────────────────────────

#[test]
fn url_encoded_traversal_denied() {
    let (_temp, g) = make_guard();
    let result = g.validate_path(Path::new("%2e%2e%2f%2e%2e%2fetc/passwd"));
    assert!(result.is_err());
}

#[test]
fn absolute_etc_path_denied() {
    let (_temp, g) = make_guard();
    assert!(g.validate_path(Path::new("/etc/shadow")).is_err());
}

#[test]
fn workspace_accessor_returns_path() {
    let (temp, g) = make_guard();
    // workspace() returns the base path
    let ws = g.workspace();
    assert!(temp.path().starts_with(ws) || ws.starts_with(temp.path()));
}
