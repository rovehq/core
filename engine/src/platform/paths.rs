use std::path::Path;

/// Get the platform-specific path separator.
pub fn path_separator() -> &'static str {
    std::path::MAIN_SEPARATOR_STR
}

/// Join path components with the platform-specific separator.
pub fn join_path(components: &[&str]) -> String {
    components.join(path_separator())
}

/// Display a path using the platform-specific separator.
pub fn display_path(path: &Path) -> String {
    path.display().to_string()
}

/// Check if the current platform is Unix-like.
pub fn is_unix() -> bool {
    cfg!(unix)
}

/// Check if the current platform is Windows.
pub fn is_windows() -> bool {
    cfg!(windows)
}

/// Get the platform name as a string.
pub fn platform_name() -> &'static str {
    #[cfg(target_os = "linux")]
    return "linux";

    #[cfg(target_os = "macos")]
    return "macos";

    #[cfg(target_os = "windows")]
    return "windows";

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return "unknown";
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{display_path, is_unix, is_windows, join_path, path_separator, platform_name};

    #[test]
    fn test_path_separator() {
        let sep = path_separator();

        #[cfg(unix)]
        assert_eq!(sep, "/");

        #[cfg(windows)]
        assert_eq!(sep, "\\");
    }

    #[test]
    fn test_join_path() {
        let path = join_path(&["home", "user", "file.txt"]);

        #[cfg(unix)]
        assert_eq!(path, "home/user/file.txt");

        #[cfg(windows)]
        assert_eq!(path, "home\\user\\file.txt");
    }

    #[test]
    fn test_display_path() {
        let path = PathBuf::from("home").join("user").join("file.txt");
        let display = display_path(&path);

        #[cfg(unix)]
        assert!(display.contains('/'));

        #[cfg(windows)]
        assert!(display.contains('\\'));
    }

    #[test]
    fn test_is_unix() {
        #[cfg(unix)]
        assert!(is_unix());

        #[cfg(windows)]
        assert!(!is_unix());
    }

    #[test]
    fn test_is_windows() {
        #[cfg(windows)]
        assert!(is_windows());

        #[cfg(unix)]
        assert!(!is_windows());
    }

    #[test]
    fn test_platform_name() {
        let name = platform_name();
        assert!(["linux", "macos", "windows", "unknown"].contains(&name));

        #[cfg(target_os = "linux")]
        assert_eq!(name, "linux");

        #[cfg(target_os = "macos")]
        assert_eq!(name, "macos");

        #[cfg(target_os = "windows")]
        assert_eq!(name, "windows");
    }
}
