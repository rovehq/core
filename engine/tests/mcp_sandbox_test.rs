use rove_engine::runtime::mcp::{McpSandbox, SandboxProfile};

#[test]
fn test_default_profile() {
    let profile = SandboxProfile::default();
    assert!(!profile.allow_network);
    assert!(profile.read_paths.is_empty());
    assert!(profile.write_paths.is_empty());
    assert!(!profile.allow_tmp);
}

#[test]
fn test_profile_builder() {
    let profile = SandboxProfile::default()
        .with_network()
        .with_read_path("/usr/local")
        .with_write_path("/tmp/output")
        .with_tmp();

    assert!(profile.allow_network);
    assert_eq!(profile.read_paths.len(), 1);
    assert_eq!(profile.write_paths.len(), 1);
    assert!(profile.allow_tmp);
}

#[test]
fn test_sandbox_availability() {
    let _ = McpSandbox::check_availability();
}

#[cfg(target_os = "linux")]
#[test]
fn test_linux_sandbox_command() {
    let profile = SandboxProfile::default();
    let result = McpSandbox::wrap_command("echo", &["hello".to_string()], &profile);

    match result {
        Ok(cmd) => assert_eq!(cmd.get_program().to_string_lossy(), "bwrap"),
        Err(_) => {}
    }
}

#[cfg(target_os = "macos")]
#[test]
fn test_macos_sandbox_command() {
    let profile = SandboxProfile::default();
    let result = McpSandbox::wrap_command("echo", &["hello".to_string()], &profile).unwrap();
    assert_eq!(result.get_program().to_string_lossy(), "sandbox-exec");
}

#[cfg(target_os = "windows")]
#[test]
fn test_windows_sandbox_command() {
    let profile = SandboxProfile::default();
    let result = McpSandbox::wrap_command(
        "cmd.exe",
        &["/c".to_string(), "echo".to_string(), "hello".to_string()],
        &profile,
    )
    .unwrap();
    assert_eq!(result.get_program().to_string_lossy(), "cmd.exe");
}

#[test]
fn test_profile_with_multiple_paths() {
    let profile = SandboxProfile::default()
        .with_read_path("/usr/local")
        .with_read_path("/opt")
        .with_write_path("/tmp/a")
        .with_write_path("/tmp/b");

    assert_eq!(profile.read_paths.len(), 2);
    assert_eq!(profile.write_paths.len(), 2);
}