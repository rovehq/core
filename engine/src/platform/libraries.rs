/// Get the platform-specific shared library extension.
pub fn library_extension() -> &'static str {
    #[cfg(target_os = "linux")]
    return "so";

    #[cfg(target_os = "macos")]
    return "dylib";

    #[cfg(target_os = "windows")]
    return "dll";

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return "so";
}

/// Get the platform-specific shared library prefix.
pub fn library_prefix() -> &'static str {
    #[cfg(unix)]
    return "lib";

    #[cfg(windows)]
    return "";
}

/// Construct a platform-specific library filename.
pub fn library_filename(name: &str) -> String {
    format!("{}{}.{}", library_prefix(), name, library_extension())
}

#[cfg(test)]
mod tests {
    use super::{library_extension, library_filename, library_prefix};

    #[test]
    fn test_library_extension() {
        let ext = library_extension();

        #[cfg(target_os = "linux")]
        assert_eq!(ext, "so");

        #[cfg(target_os = "macos")]
        assert_eq!(ext, "dylib");

        #[cfg(target_os = "windows")]
        assert_eq!(ext, "dll");
    }

    #[test]
    fn test_library_prefix() {
        let prefix = library_prefix();

        #[cfg(unix)]
        assert_eq!(prefix, "lib");

        #[cfg(windows)]
        assert_eq!(prefix, "");
    }

    #[test]
    fn test_library_filename() {
        let filename = library_filename("telegram");

        #[cfg(target_os = "linux")]
        assert_eq!(filename, "libtelegram.so");

        #[cfg(target_os = "macos")]
        assert_eq!(filename, "libtelegram.dylib");

        #[cfg(target_os = "windows")]
        assert_eq!(filename, "telegram.dll");
    }

    #[test]
    fn test_library_filename_with_special_chars() {
        let filename = library_filename("ui-server");

        #[cfg(target_os = "linux")]
        assert_eq!(filename, "libui-server.so");

        #[cfg(target_os = "macos")]
        assert_eq!(filename, "libui-server.dylib");

        #[cfg(target_os = "windows")]
        assert_eq!(filename, "ui-server.dll");
    }
}
