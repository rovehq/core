/// Platform-specific line ending.
#[cfg(unix)]
pub const LINE_ENDING: &str = "\n";

/// Platform-specific line ending.
#[cfg(windows)]
pub const LINE_ENDING: &str = "\r\n";

/// Normalize line endings in text to the platform-specific format.
pub fn normalize_line_endings(text: &str) -> String {
    #[cfg(unix)]
    {
        text.replace("\r\n", "\n")
    }

    #[cfg(windows)]
    {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    }
}

/// Convert line endings to Unix format (LF).
pub fn to_unix_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Convert line endings to Windows format (CRLF).
pub fn to_windows_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::{normalize_line_endings, to_unix_line_endings, to_windows_line_endings, LINE_ENDING};

    #[test]
    fn test_line_ending_constant() {
        #[cfg(unix)]
        assert_eq!(LINE_ENDING, "\n");

        #[cfg(windows)]
        assert_eq!(LINE_ENDING, "\r\n");
    }

    #[test]
    fn test_normalize_line_endings_mixed() {
        let text = "line1\r\nline2\nline3\r\n";
        let normalized = normalize_line_endings(text);

        #[cfg(unix)]
        assert_eq!(normalized, "line1\nline2\nline3\n");

        #[cfg(windows)]
        assert_eq!(normalized, "line1\r\nline2\r\nline3\r\n");
    }

    #[test]
    fn test_normalize_line_endings_already_normalized() {
        #[cfg(unix)]
        {
            let text = "line1\nline2\nline3\n";
            let normalized = normalize_line_endings(text);
            assert_eq!(normalized, text);
        }

        #[cfg(windows)]
        {
            let text = "line1\r\nline2\r\nline3\r\n";
            let normalized = normalize_line_endings(text);
            assert_eq!(normalized, text);
        }
    }

    #[test]
    fn test_to_unix_line_endings() {
        let text = "line1\r\nline2\nline3\r\n";
        let unix = to_unix_line_endings(text);
        assert_eq!(unix, "line1\nline2\nline3\n");
    }

    #[test]
    fn test_to_windows_line_endings() {
        let text = "line1\nline2\r\nline3\n";
        let windows = to_windows_line_endings(text);
        assert_eq!(windows, "line1\r\nline2\r\nline3\r\n");
    }

    #[test]
    fn test_round_trip_line_endings() {
        let original = "line1\nline2\nline3\n";
        let windows = to_windows_line_endings(original);
        let back_to_unix = to_unix_line_endings(&windows);
        assert_eq!(back_to_unix, original);
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(normalize_line_endings(""), "");
        assert_eq!(to_unix_line_endings(""), "");
        assert_eq!(to_windows_line_endings(""), "");
    }

    #[test]
    fn test_no_line_endings() {
        let text = "single line with no ending";
        assert_eq!(normalize_line_endings(text), text);
        assert_eq!(to_unix_line_endings(text), text);
        assert_eq!(to_windows_line_endings(text), text);
    }
}
