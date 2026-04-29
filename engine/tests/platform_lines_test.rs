//! Tests for platform::lines — normalize_line_endings, to_unix_line_endings, to_windows_line_endings

use rove_engine::platform::lines::{
    normalize_line_endings, to_unix_line_endings, to_windows_line_endings, LINE_ENDING,
};

// ── LINE_ENDING constant ─────────────────────────────────────────────────────

#[test]
fn line_ending_is_unix_on_unix() {
    #[cfg(unix)]
    assert_eq!(LINE_ENDING, "\n");
}

#[test]
fn line_ending_is_windows_on_windows() {
    #[cfg(windows)]
    assert_eq!(LINE_ENDING, "\r\n");
}

#[test]
fn line_ending_not_empty() {
    assert!(!LINE_ENDING.is_empty());
}

// ── to_unix_line_endings ──────────────────────────────────────────────────────

#[test]
fn to_unix_crlf_becomes_lf() {
    assert_eq!(to_unix_line_endings("a\r\nb"), "a\nb");
}

#[test]
fn to_unix_already_lf_unchanged() {
    assert_eq!(to_unix_line_endings("a\nb\nc"), "a\nb\nc");
}

#[test]
fn to_unix_empty_string() {
    assert_eq!(to_unix_line_endings(""), "");
}

#[test]
fn to_unix_no_newlines() {
    assert_eq!(to_unix_line_endings("no newlines here"), "no newlines here");
}

#[test]
fn to_unix_multiple_crlf() {
    assert_eq!(to_unix_line_endings("a\r\nb\r\nc\r\n"), "a\nb\nc\n");
}

#[test]
fn to_unix_mixed_endings() {
    let input = "line1\r\nline2\nline3\r\n";
    assert_eq!(to_unix_line_endings(input), "line1\nline2\nline3\n");
}

#[test]
fn to_unix_only_crlf() {
    assert_eq!(to_unix_line_endings("\r\n"), "\n");
}

#[test]
fn to_unix_only_lf() {
    assert_eq!(to_unix_line_endings("\n"), "\n");
}

#[test]
fn to_unix_single_char() {
    assert_eq!(to_unix_line_endings("x"), "x");
}

#[test]
fn to_unix_trailing_crlf() {
    assert_eq!(to_unix_line_endings("hello\r\n"), "hello\n");
}

#[test]
fn to_unix_leading_crlf() {
    assert_eq!(to_unix_line_endings("\r\nhello"), "\nhello");
}

#[test]
fn to_unix_many_blank_lines() {
    assert_eq!(to_unix_line_endings("\r\n\r\n\r\n"), "\n\n\n");
}

#[test]
fn to_unix_preserves_cr_only() {
    // Bare CR without LF should be left alone
    assert_eq!(to_unix_line_endings("a\rb"), "a\rb");
}

#[test]
fn to_unix_long_content() {
    let long = "hello world\r\n".repeat(1000);
    let result = to_unix_line_endings(&long);
    assert!(!result.contains('\r'));
    assert!(result.contains('\n'));
}

// ── to_windows_line_endings ───────────────────────────────────────────────────

#[test]
fn to_windows_lf_becomes_crlf() {
    assert_eq!(to_windows_line_endings("a\nb"), "a\r\nb");
}

#[test]
fn to_windows_empty_string() {
    assert_eq!(to_windows_line_endings(""), "");
}

#[test]
fn to_windows_no_newlines() {
    assert_eq!(to_windows_line_endings("no newlines"), "no newlines");
}

#[test]
fn to_windows_multiple_lf() {
    assert_eq!(to_windows_line_endings("a\nb\nc\n"), "a\r\nb\r\nc\r\n");
}

#[test]
fn to_windows_already_crlf_stays_crlf() {
    let input = "a\r\nb\r\nc";
    let result = to_windows_line_endings(input);
    // CRLF must not become CRCRLF (double-conversion guard)
    assert_eq!(result, "a\r\nb\r\nc");
}

#[test]
fn to_windows_only_lf() {
    assert_eq!(to_windows_line_endings("\n"), "\r\n");
}

#[test]
fn to_windows_only_crlf() {
    assert_eq!(to_windows_line_endings("\r\n"), "\r\n");
}

#[test]
fn to_windows_trailing_lf() {
    assert_eq!(to_windows_line_endings("hello\n"), "hello\r\n");
}

#[test]
fn to_windows_leading_lf() {
    assert_eq!(to_windows_line_endings("\nhello"), "\r\nhello");
}

#[test]
fn to_windows_many_blank_lines_lf() {
    assert_eq!(to_windows_line_endings("\n\n\n"), "\r\n\r\n\r\n");
}

#[test]
fn to_windows_mixed_input_crlf_first() {
    // Input has both CRLF and bare LF
    let input = "a\r\nb\nc";
    let result = to_windows_line_endings(input);
    assert_eq!(result, "a\r\nb\r\nc");
}

#[test]
fn to_windows_long_content() {
    let long = "hello world\n".repeat(1000);
    let result = to_windows_line_endings(&long);
    assert!(result.contains("\r\n"));
    assert_eq!(result.matches("\r\n").count(), 1000);
}

// ── normalize_line_endings ────────────────────────────────────────────────────

#[test]
fn normalize_empty() {
    assert_eq!(normalize_line_endings(""), "");
}

#[test]
fn normalize_no_newlines() {
    assert_eq!(normalize_line_endings("plain text"), "plain text");
}

#[test]
fn normalize_crlf_to_platform() {
    let result = normalize_line_endings("a\r\nb");
    #[cfg(unix)]
    assert_eq!(result, "a\nb");
    #[cfg(windows)]
    assert_eq!(result, "a\r\nb");
}

#[test]
fn normalize_pure_lf_on_unix_unchanged() {
    #[cfg(unix)]
    assert_eq!(normalize_line_endings("a\nb\nc"), "a\nb\nc");
}

#[test]
fn normalize_pure_crlf_on_windows_unchanged() {
    #[cfg(windows)]
    assert_eq!(normalize_line_endings("a\r\nb\r\nc"), "a\r\nb\r\nc");
}

#[test]
fn normalize_already_correct_no_change() {
    // Whatever platform we're on, passing already-normalized text
    // should produce identical output
    let text = "line1\nline2\nline3";
    let normalized = normalize_line_endings(text);
    #[cfg(unix)]
    assert_eq!(normalized, text);
}

#[test]
fn normalize_multiple_lines_crlf() {
    let text = "first\r\nsecond\r\nthird\r\n";
    let normalized = normalize_line_endings(text);
    #[cfg(unix)]
    assert_eq!(normalized, "first\nsecond\nthird\n");
}

// ── Round-trip tests ──────────────────────────────────────────────────────────

#[test]
fn round_trip_unix_to_windows_to_unix() {
    let original = "alpha\nbeta\ngamma\n";
    let win = to_windows_line_endings(original);
    let back = to_unix_line_endings(&win);
    assert_eq!(back, original);
}

#[test]
fn round_trip_windows_to_unix_to_windows() {
    let original = "alpha\r\nbeta\r\ngamma\r\n";
    let unix = to_unix_line_endings(original);
    let back = to_windows_line_endings(&unix);
    assert_eq!(back, original);
}

#[test]
fn round_trip_empty() {
    let original = "";
    let win = to_windows_line_endings(original);
    let back = to_unix_line_endings(&win);
    assert_eq!(back, original);
}

#[test]
fn round_trip_no_newlines() {
    let original = "no line endings";
    let win = to_windows_line_endings(original);
    let back = to_unix_line_endings(&win);
    assert_eq!(back, original);
}

#[test]
fn round_trip_multiline_code() {
    let code = "fn main() {\n    println!(\"hello\");\n}\n";
    let win = to_windows_line_endings(code);
    let back = to_unix_line_endings(&win);
    assert_eq!(back, code);
}

#[test]
fn round_trip_json_content() {
    let json = "{\n  \"key\": \"value\",\n  \"num\": 42\n}\n";
    let win = to_windows_line_endings(json);
    let back = to_unix_line_endings(&win);
    assert_eq!(back, json);
}

// ── Edge cases ────────────────────────────────────────────────────────────────

#[test]
fn to_unix_null_bytes_unaffected() {
    let input = "a\x00b\r\nc";
    let result = to_unix_line_endings(input);
    assert_eq!(result, "a\x00b\nc");
}

#[test]
fn to_windows_null_bytes_unaffected() {
    let input = "a\x00b\nc";
    let result = to_windows_line_endings(input);
    assert_eq!(result, "a\x00b\r\nc");
}

#[test]
fn to_unix_unicode_content_preserved() {
    let input = "héllo\r\nwörld\r\n";
    let result = to_unix_line_endings(input);
    assert_eq!(result, "héllo\nwörld\n");
}

#[test]
fn to_windows_unicode_content_preserved() {
    let input = "héllo\nwörld\n";
    let result = to_windows_line_endings(input);
    assert_eq!(result, "héllo\r\nwörld\r\n");
}

#[test]
fn to_unix_single_newline() {
    assert_eq!(to_unix_line_endings("\r\n"), "\n");
}

#[test]
fn to_windows_single_newline() {
    assert_eq!(to_windows_line_endings("\n"), "\r\n");
}

#[test]
fn normalize_single_newline() {
    let result = normalize_line_endings("\r\n");
    #[cfg(unix)]
    assert_eq!(result, "\n");
    #[cfg(windows)]
    assert_eq!(result, "\r\n");
}

#[test]
fn to_unix_tab_preserved() {
    assert_eq!(to_unix_line_endings("\ta\r\n\tb"), "\ta\n\tb");
}

#[test]
fn to_windows_tab_preserved() {
    assert_eq!(to_windows_line_endings("\ta\n\tb"), "\ta\r\n\tb");
}

#[test]
fn to_unix_exactly_64_lines() {
    let input = "x\r\n".repeat(64);
    let result = to_unix_line_endings(&input);
    assert_eq!(result.lines().count(), 64);
    assert!(!result.contains('\r'));
}

#[test]
fn to_windows_exactly_64_lines() {
    let input = "x\n".repeat(64);
    let result = to_windows_line_endings(&input);
    assert_eq!(result.matches("\r\n").count(), 64);
}
