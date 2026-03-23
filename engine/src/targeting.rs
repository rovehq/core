pub fn extract_task_target(input: &str) -> (String, Option<String>) {
    let trimmed = input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    for marker in [" in ", " on "] {
        if let Some(index) = lowered.rfind(marker) {
            let prompt = trimmed[..index].trim();
            let node = trimmed[index + marker.len()..].trim();
            if !prompt.is_empty()
                && !node.is_empty()
                && node
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
            {
                return (prompt.to_string(), Some(node.to_string()));
            }
        }
    }

    (trimmed.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::extract_task_target;

    #[test]
    fn extracts_node_from_in_suffix() {
        let (task, node) = extract_task_target("do that in home-mac");
        assert_eq!(task, "do that");
        assert_eq!(node.as_deref(), Some("home-mac"));
    }

    #[test]
    fn extracts_node_from_on_suffix_case_insensitively() {
        let (task, node) = extract_task_target("deploy branch ON office_mac");
        assert_eq!(task, "deploy branch");
        assert_eq!(node.as_deref(), Some("office_mac"));
    }

    #[test]
    fn ignores_invalid_node_suffix() {
        let (task, node) = extract_task_target("do that in home mac!");
        assert_eq!(task, "do that in home mac!");
        assert_eq!(node, None);
    }
}
