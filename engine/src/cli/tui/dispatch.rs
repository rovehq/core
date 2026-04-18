use anyhow::Result;

fn is_trace_line(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() > 24
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && (line.contains("  INFO ")
            || line.contains("  WARN ")
            || line.contains("  ERROR")
            || line.contains("  DEBUG")
            || line.contains("  TRACE"))
}

pub async fn run(full_cmd: &str) -> Result<Vec<String>> {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("rove"));
    let argv: Vec<&str> = full_cmd.split_whitespace().collect();

    let output = tokio::process::Command::new(&exe)
        .args(&argv)
        .output()
        .await?;

    let mut lines = Vec::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        lines.push(line.to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        if !is_trace_line(line) {
            lines.push(line.to_string());
        }
    }

    Ok(lines)
}
