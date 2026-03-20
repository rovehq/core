use sdk::Complexity;

use super::Classification;

pub(super) fn classify_with_heuristics(input: &str) -> Classification {
    let input_lower = input.to_lowercase();

    let (domain_label, domain_confidence) = if input_lower.contains("git")
        || input_lower.contains("commit")
        || input_lower.contains("branch")
        || input_lower.contains("rebase")
        || input_lower.contains("squash")
    {
        ("git".to_string(), 0.97)
    } else if input_lower.contains("ls")
        || input_lower.contains("cd")
        || input_lower.contains("mkdir")
        || input_lower.contains("terminal")
        || input_lower.contains("list files")
        || input_lower.contains("directory")
        || input_lower.contains("grep")
    {
        ("shell".to_string(), 0.92)
    } else if input_lower.contains("cargo")
        || input_lower.contains("rust")
        || input_lower.contains("code")
        || input_lower.contains("function")
        || input_lower.contains("refactor")
        || input_lower.contains("test")
    {
        ("code".to_string(), 0.88)
    } else if input_lower.contains("browser")
        || input_lower.contains("web")
        || input_lower.contains("http")
        || input_lower.contains("website")
    {
        ("browser".to_string(), 0.83)
    } else if input_lower.contains("search")
        || input_lower.contains("research")
        || input_lower.contains("compare pricing")
    {
        ("search".to_string(), 0.8)
    } else if input_lower.contains("data")
        || input_lower.contains("csv")
        || input_lower.contains("json")
        || input_lower.contains("sql")
    {
        ("data".to_string(), 0.78)
    } else {
        ("general".to_string(), 0.25)
    };

    let complexity = if input_lower.contains("plan")
        || input_lower.contains("multi-step")
        || input_lower.contains("complex")
        || input_lower.contains("parallel")
        || input_lower.contains("dag")
    {
        Complexity::Complex
    } else if input_lower.contains("then")
        || input_lower.contains("after")
        || input_lower.contains("and then")
    {
        Complexity::Medium
    } else {
        Complexity::Simple
    };

    let sensitive = input_lower.contains("password")
        || input_lower.contains("secret")
        || input_lower.contains("token")
        || input_lower.contains("api key")
        || input_lower.contains("credential")
        || input_lower.contains("private key");

    let injection_score = if input_lower.contains("ignore previous")
        || input_lower.contains("system prompt")
        || input_lower.contains("developer message")
        || input_lower.contains("reveal hidden")
    {
        0.95
    } else if input_lower.contains("bypass")
        || input_lower.contains("override")
        || input_lower.contains("disable safety")
    {
        0.75
    } else {
        0.05
    };

    Classification {
        domain_label,
        domain_confidence,
        complexity,
        sensitive,
        injection_score,
    }
}
