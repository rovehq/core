//! Integration tests for platform-specific path and line ending handling
//!
//! These tests verify that the engine correctly handles platform-specific
//! differences in path separators and line endings.
//!
//! Requirements:
//! - 25.2: Use platform-specific paths (/ on Unix, \ on Windows)
//! - 25.5: Handle platform-specific line endings (LF on Unix, CRLF on Windows)

use rove_engine::config::Config;
use rove_engine::fs_guard::FileSystemGuard;
use rove_engine::platform::{
    display_path, is_unix, is_windows, join_path, normalize_line_endings, path_separator,
    platform_name, to_unix_line_endings, to_windows_line_endings, LINE_ENDING,
};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_path_separator_matches_platform() {
    // Verify that the path separator matches the current platform
    let sep = path_separator();

    if cfg!(unix) {
        assert_eq!(sep, "/", "Unix platforms should use forward slash");
    } else if cfg!(windows) {
        assert_eq!(sep, "\\", "Windows should use backslash");
    }
}

#[test]
fn test_line_ending_matches_platform() {
    // Verify that the line ending constant matches the current platform
    if cfg!(unix) {
        assert_eq!(LINE_ENDING, "\n", "Unix platforms should use LF");
    } else if cfg!(windows) {
        assert_eq!(LINE_ENDING, "\r\n", "Windows should use CRLF");
    }
}

#[test]
fn test_pathbuf_uses_platform_separator() {
    // Verify that PathBuf automatically uses the correct separator
    let path = PathBuf::from("home").join("user").join("file.txt");
    let display = path.display().to_string();

    if cfg!(unix) {
        assert!(
            display.contains('/'),
            "Unix paths should contain forward slashes"
        );
        assert!(
            !display.contains('\\'),
            "Unix paths should not contain backslashes"
        );
    } else if cfg!(windows) {
        assert!(
            display.contains('\\'),
            "Windows paths should contain backslashes"
        );
    }
}

#[test]
fn test_fs_guard_with_platform_paths() {
    // Create a temporary workspace
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().to_path_buf();
    let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

    // Create a test file using PathBuf (platform-agnostic)
    let test_file = workspace.join("subdir").join("test.txt");
    fs::create_dir_all(test_file.parent().unwrap()).unwrap();
    fs::write(&test_file, "test content").unwrap();

    // Validate the path - should work regardless of platform
    let validated = guard.validate_path(&test_file);
    assert!(
        validated.is_ok(),
        "FileSystemGuard should validate paths correctly on all platforms"
    );

    // The validated path should be canonical and use platform separators
    let canonical = validated.unwrap();
    let display = canonical.display().to_string();

    if cfg!(unix) {
        assert!(display.contains('/'), "Canonical Unix paths should use /");
    } else if cfg!(windows) {
        assert!(
            display.contains('\\'),
            "Canonical Windows paths should use \\"
        );
    }
}

#[test]
fn test_config_path_expansion() {
    // Create a temporary config file
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("config.toml");

    // Create a workspace path using platform-agnostic PathBuf
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    let config_content = format!(
        r#"
[core]
workspace = '{}'
log_level = "info"
auto_sync = true
data_dir = '{}'

[llm]
default_provider = "ollama"

[tools]
tg-controller = false
ui-server = false
api-server = false

[plugins]
fs-editor = true
terminal = true
screenshot = false
git = true

[security]
max_risk_tier = 2
confirm_tier1 = true
confirm_tier1_delay = 10
require_explicit_tier2 = true
"#,
        workspace.display(),
        temp.path().join("data").display()
    );

    fs::write(&config_path, config_content).unwrap();

    // Load the config - paths should be canonicalized with platform separators
    let config = Config::load_from_path(&config_path).unwrap();

    // Verify the workspace path is canonical
    assert!(config.core.workspace.is_absolute());
    assert!(config.core.workspace.exists());

    // Verify the path display uses platform separators
    let display = config.core.workspace.display().to_string();
    if cfg!(unix) {
        assert!(display.contains('/'));
    } else if cfg!(windows) {
        assert!(display.contains('\\'));
    }
}

#[test]
fn test_normalize_line_endings_for_platform() {
    let mixed_text = "line1\r\nline2\nline3\r\n";
    let normalized = normalize_line_endings(mixed_text);

    if cfg!(unix) {
        // On Unix, should convert to LF
        assert_eq!(normalized, "line1\nline2\nline3\n");
        assert!(!normalized.contains("\r\n"));
    } else if cfg!(windows) {
        // On Windows, should convert to CRLF
        assert_eq!(normalized, "line1\r\nline2\r\nline3\r\n");
        assert!(normalized.contains("\r\n"));
    }
}

#[test]
fn test_line_ending_conversion_round_trip() {
    let original = "line1\nline2\nline3\n";

    // Convert to Windows format
    let windows = to_windows_line_endings(original);
    assert_eq!(windows, "line1\r\nline2\r\nline3\r\n");

    // Convert back to Unix format
    let unix = to_unix_line_endings(&windows);
    assert_eq!(unix, original);
}

#[test]
fn test_file_write_with_platform_line_endings() {
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("test.txt");

    // Write content with platform-specific line endings
    let content = format!(
        "line1{}line2{}line3{}",
        LINE_ENDING, LINE_ENDING, LINE_ENDING
    );
    fs::write(&file_path, &content).unwrap();

    // Read it back
    let read_content = fs::read_to_string(&file_path).unwrap();

    // On Unix, the file should contain LF
    // On Windows, the file should contain CRLF
    if cfg!(unix) {
        assert!(read_content.contains('\n'));
        assert_eq!(read_content, "line1\nline2\nline3\n");
    } else if cfg!(windows) {
        assert!(read_content.contains("\r\n"));
        assert_eq!(read_content, "line1\r\nline2\r\nline3\r\n");
    }
}

#[test]
fn test_join_path_with_platform_separator() {
    let path = join_path(&["home", "user", "documents", "file.txt"]);

    if cfg!(unix) {
        assert_eq!(path, "home/user/documents/file.txt");
    } else if cfg!(windows) {
        assert_eq!(path, "home\\user\\documents\\file.txt");
    }
}

#[test]
fn test_display_path_with_platform_separator() {
    let path = PathBuf::from("home")
        .join("user")
        .join("documents")
        .join("file.txt");
    let display = display_path(&path);

    if cfg!(unix) {
        assert!(display.contains('/'));
        assert_eq!(display, "home/user/documents/file.txt");
    } else if cfg!(windows) {
        assert!(display.contains('\\'));
        assert_eq!(display, "home\\user\\documents\\file.txt");
    }
}

#[test]
fn test_platform_detection() {
    // Verify platform detection functions work correctly
    if cfg!(unix) {
        assert!(is_unix());
        assert!(!is_windows());
    } else if cfg!(windows) {
        assert!(is_windows());
        assert!(!is_unix());
    }

    // Verify platform name
    let name = platform_name();
    assert!(["linux", "macos", "windows", "unknown"].contains(&name));

    #[cfg(target_os = "linux")]
    assert_eq!(name, "linux");

    #[cfg(target_os = "macos")]
    assert_eq!(name, "macos");

    #[cfg(target_os = "windows")]
    assert_eq!(name, "windows");
}

#[test]
fn test_path_traversal_with_platform_separators() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

    // Create a file outside the workspace
    let outside_file = temp.path().join("secret.txt");
    fs::write(&outside_file, "secret").unwrap();

    // Try to access it via path traversal using platform-agnostic PathBuf
    let traversal_path = workspace.join("..").join("secret.txt");

    // Should be rejected regardless of platform
    let result = guard.validate_path(&traversal_path);
    assert!(
        result.is_err(),
        "Path traversal should be blocked on all platforms"
    );
}

#[test]
fn test_symlink_handling_on_unix() {
    // This test only runs on Unix systems where symlinks are well-supported
    #[cfg(unix)]
    {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let guard = FileSystemGuard::new(workspace.clone()).expect("test workspace");

        // Create a sensitive directory
        let ssh_dir = temp.path().join(".ssh");
        fs::create_dir(&ssh_dir).unwrap();

        // Create a symlink to it
        let symlink_path = workspace.join("safe_link");
        std::os::unix::fs::symlink(&ssh_dir, &symlink_path).unwrap();

        // Should be rejected after canonicalization
        let result = guard.validate_path(&symlink_path);
        assert!(
            result.is_err(),
            "Symlink to sensitive path should be blocked"
        );
    }
}

#[test]
fn test_cross_platform_path_compatibility() {
    // Verify that paths created on one platform can be understood on another
    // (when using PathBuf, not string manipulation)

    let path = PathBuf::from("home").join("user").join("file.txt");

    // PathBuf should work correctly regardless of how it's displayed
    assert_eq!(path.file_name().unwrap(), "file.txt");
    assert_eq!(path.parent().unwrap().file_name().unwrap(), "user");

    // Components should be accessible regardless of separator
    let components: Vec<_> = path.components().collect();
    assert_eq!(components.len(), 3);
}

#[test]
fn test_library_extension_matches_platform() {
    use rove_engine::platform::library_extension;

    let ext = library_extension();

    #[cfg(target_os = "linux")]
    assert_eq!(ext, "so", "Linux should use .so extension");

    #[cfg(target_os = "macos")]
    assert_eq!(ext, "dylib", "macOS should use .dylib extension");

    #[cfg(target_os = "windows")]
    assert_eq!(ext, "dll", "Windows should use .dll extension");
}

#[test]
fn test_library_prefix_matches_platform() {
    use rove_engine::platform::library_prefix;

    let prefix = library_prefix();

    #[cfg(unix)]
    assert_eq!(prefix, "lib", "Unix platforms should use 'lib' prefix");

    #[cfg(windows)]
    assert_eq!(prefix, "", "Windows should not use a prefix");
}

#[test]
fn test_library_filename_construction() {
    use rove_engine::platform::library_filename;

    let filename = library_filename("telegram");

    // Verify the filename contains the correct components
    #[cfg(target_os = "linux")]
    {
        assert_eq!(filename, "libtelegram.so");
        assert!(filename.starts_with("lib"));
        assert!(filename.ends_with(".so"));
    }

    #[cfg(target_os = "macos")]
    {
        assert_eq!(filename, "libtelegram.dylib");
        assert!(filename.starts_with("lib"));
        assert!(filename.ends_with(".dylib"));
    }

    #[cfg(target_os = "windows")]
    {
        assert_eq!(filename, "telegram.dll");
        assert!(!filename.starts_with("lib"));
        assert!(filename.ends_with(".dll"));
    }
}

#[test]
fn test_library_filename_with_hyphens() {
    use rove_engine::platform::library_filename;

    // Test with a name containing hyphens (common in Rust crate names)
    let filename = library_filename("ui-server");

    #[cfg(target_os = "linux")]
    assert_eq!(filename, "libui-server.so");

    #[cfg(target_os = "macos")]
    assert_eq!(filename, "libui-server.dylib");

    #[cfg(target_os = "windows")]
    assert_eq!(filename, "ui-server.dll");
}

#[test]
fn test_library_filename_with_underscores() {
    use rove_engine::platform::library_filename;

    // Test with a name containing underscores
    let filename = library_filename("api_server");

    #[cfg(target_os = "linux")]
    assert_eq!(filename, "libapi_server.so");

    #[cfg(target_os = "macos")]
    assert_eq!(filename, "libapi_server.dylib");

    #[cfg(target_os = "windows")]
    assert_eq!(filename, "api_server.dll");
}
