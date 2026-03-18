//! LLM Router
//!
//! Intelligently selects which LLM provider to use based on task characteristics.
//! The router analyzes task sensitivity, complexity, and token requirements to rank
//! providers and select the most appropriate one.
//!
//! **Requirements**: 4.2, 4.3, 4.6

use super::{LLMProvider, Message};
use crate::config::LLMConfig;
use crate::security::secrets::scrub_text;
use brain::reasoning::LocalBrain;
use sdk::Brain;
use std::sync::Arc;
use std::time::Duration;

/// Task profile used for provider ranking
#[derive(Debug, Clone)]
pub struct TaskProfile {
    /// Sensitivity score (0.0-1.0)
    /// Higher values indicate more sensitive data
    pub sensitivity: f64,

    /// Complexity score (0.0-1.0)
    /// Higher values indicate more complex tasks requiring larger context
    pub complexity: f64,

    /// Estimated token count for the task
    pub estimated_tokens: usize,
}

impl TaskProfile {
    /// Create a new task profile
    pub fn new(sensitivity: f64, complexity: f64, estimated_tokens: usize) -> Self {
        Self {
            sensitivity: sensitivity.clamp(0.0, 1.0),
            complexity: complexity.clamp(0.0, 1.0),
            estimated_tokens,
        }
    }
}

/// LLM Router that selects appropriate providers based on task characteristics
pub struct LLMRouter {
    /// Available LLM providers
    providers: Vec<Box<dyn LLMProvider>>,

    /// LLM configuration
    config: Arc<LLMConfig>,

    /// Optional local reasoning brain
    local_brain: Option<Arc<LocalBrain>>,
}

impl LLMRouter {
    /// Create a new LLM router
    ///
    /// # Arguments
    /// * `providers` - List of available LLM providers
    /// * `config` - LLM configuration
    pub fn new(providers: Vec<Box<dyn LLMProvider>>, config: Arc<LLMConfig>) -> Self {
        Self {
            providers,
            config,
            local_brain: None,
        }
    }

    /// Create a new LLM router with optional local brain
    ///
    /// # Arguments
    /// * `providers` - List of available LLM providers
    /// * `config` - LLM configuration
    /// * `local_brain` - Optional local reasoning brain
    pub fn with_local_brain(
        providers: Vec<Box<dyn LLMProvider>>,
        config: Arc<LLMConfig>,
        local_brain: Option<Arc<LocalBrain>>,
    ) -> Self {
        Self {
            providers,
            config,
            local_brain,
        }
    }

    /// Analyze task characteristics from message history
    ///
    /// This method examines the conversation history to determine:
    /// - Sensitivity: Presence of keywords indicating sensitive data
    /// - Complexity: Length and structure of the conversation
    /// - Token count: Estimated tokens needed for the task
    ///
    /// **Validates: Requirements 4.2**
    pub fn analyze_task(&self, messages: &[Message]) -> TaskProfile {
        let sensitivity = self.calculate_sensitivity(messages);
        let complexity = self.calculate_complexity(messages);
        let estimated_tokens = self.estimate_tokens(messages);

        TaskProfile::new(sensitivity, complexity, estimated_tokens)
    }

    /// Calculate sensitivity score based on message content
    ///
    /// Scans for keywords that indicate sensitive data:
    /// - Credentials, passwords, keys, tokens
    /// - Personal information (SSN, credit card, etc.)
    /// - Private file paths (.ssh, .env, etc.)
    ///
    /// **Validates: Requirements 4.2**
    fn calculate_sensitivity(&self, messages: &[Message]) -> f64 {
        const SENSITIVE_KEYWORDS: &[&str] = &[
            "password",
            "credential",
            "secret",
            "token",
            "api_key",
            "private_key",
            "ssh",
            ".env",
            "ssn",
            "credit_card",
            "bank",
            "account",
        ];

        let mut sensitivity_score: f64 = 0.0;
        let total_content: String = messages
            .iter()
            .map(|m| m.content.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");

        for keyword in SENSITIVE_KEYWORDS {
            if total_content.contains(keyword) {
                sensitivity_score += 0.2;
            }
        }

        // Cap at 1.0
        sensitivity_score.min(1.0)
    }

    /// Calculate complexity score based on conversation structure
    ///
    /// Factors considered:
    /// - Number of messages (more messages = more complex)
    /// - Average message length (longer messages = more complex)
    /// - Presence of code blocks or structured data
    ///
    /// **Validates: Requirements 4.3**
    fn calculate_complexity(&self, messages: &[Message]) -> f64 {
        if messages.is_empty() {
            return 0.0;
        }

        let message_count = messages.len();
        let total_length: usize = messages.iter().map(|m| m.content.len()).sum();
        let avg_length = total_length / message_count;

        // Base complexity from message count (more messages = more context needed)
        let count_complexity = (message_count as f64 / 10.0).min(0.5);

        // Complexity from average message length
        let length_complexity = (avg_length as f64 / 1000.0).min(0.3);

        // Check for code blocks or structured data
        let has_code_blocks = messages
            .iter()
            .any(|m| m.content.contains("```") || m.content.contains("```"));
        let code_complexity = if has_code_blocks { 0.2 } else { 0.0 };

        (count_complexity + length_complexity + code_complexity).min(1.0)
    }

    /// Estimate token count for the conversation
    ///
    /// Uses a simple heuristic: ~4 characters per token
    /// This is a rough approximation that works reasonably well for English text
    fn estimate_tokens(&self, messages: &[Message]) -> usize {
        let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        total_chars / 4
    }

    /// Rank providers based on task profile
    ///
    /// Ranking algorithm:
    /// 1. Prefer local providers for sensitive tasks (sensitivity > threshold)
    /// 2. Prefer cloud providers for complex tasks (complexity > threshold)
    /// 3. Prefer cloud providers for large tasks (tokens > session limit)
    /// 4. Consider cost (prefer cheaper when quality is similar)
    ///
    /// Returns a sorted list of providers (best first)
    ///
    /// **Validates: Requirements 4.2, 4.3, 4.6**
    pub fn rank_providers(&self, profile: &TaskProfile) -> Vec<&dyn LLMProvider> {
        let mut providers: Vec<&dyn LLMProvider> =
            self.providers.iter().map(|b| b.as_ref()).collect();

        let default_provider = &self.config.default_provider;

        providers.sort_by(|a, b| {
            let mut score_a = 0.0_f64;
            let mut score_b = 0.0_f64;

            // Strongly prefer the user's configured default provider
            if a.name() == default_provider {
                score_a += 200.0;
            }
            if b.name() == default_provider {
                score_b += 200.0;
            }

            // Prefer local for sensitive tasks
            if profile.sensitivity > self.config.sensitivity_threshold {
                if a.is_local() {
                    score_a += 100.0;
                }
                if b.is_local() {
                    score_b += 100.0;
                }
            }

            // Prefer cloud for complex tasks (larger context windows)
            if profile.complexity > self.config.complexity_threshold {
                if !a.is_local() {
                    score_a += 500.0;
                }
                if !b.is_local() {
                    score_b += 500.0;
                }
            }

            // Prefer cloud heavily if token estimate is high (e.g., > 4000)
            if profile.estimated_tokens > 4000 {
                if !a.is_local() {
                    score_a += 50.0;
                }
                if !b.is_local() {
                    score_b += 50.0;
                }
            }

            // Consider cost (multiply by 1000 to make it significant)
            // Lower cost = higher score
            let cost_a = a.estimated_cost(profile.estimated_tokens);
            let cost_b = b.estimated_cost(profile.estimated_tokens);
            score_a -= cost_a * 1000.0;
            score_b -= cost_b * 1000.0;

            // Sort by score descending (higher score first)
            // Use partial_cmp and unwrap_or for f64 comparison
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        providers
    }

    /// Call LLM providers with automatic failover
    ///
    /// This method:
    /// 1. Checks if LocalBrain is available and tries it first
    /// 2. Analyzes the task to create a profile
    /// 3. Ranks providers based on the profile
    /// 4. Attempts providers in order with 30-second timeout each
    /// 5. Returns AllProvidersExhausted if all fail
    ///
    /// Requirements: 4.4, 4.5
    pub async fn call(&self, messages: &[Message]) -> super::Result<(super::LLMResponse, String)> {
        use super::LLMError;

        // Try LocalBrain first if available
        if let Some(local_brain) = &self.local_brain {
            if local_brain.check_available().await {
                tracing::debug!("Attempting LocalBrain (llama-server)");

                // Convert messages to Brain trait format
                let brain_messages: Vec<sdk::Message> = messages
                    .iter()
                    .map(|m| sdk::Message {
                        role: m.role.to_string(),
                        content: m.content.clone(),
                    })
                    .collect();

                // Try LocalBrain with timeout
                let result = tokio::time::timeout(
                    Duration::from_secs(120),
                    local_brain.complete("You are a helpful AI assistant.", &brain_messages, &[]),
                )
                .await;

                match result {
                    Ok(Ok(brain_response)) => {
                        tracing::info!("LocalBrain succeeded");
                        // Convert BrainResponse to LLMResponse
                        let llm_response = super::LLMResponse::FinalAnswer(super::FinalAnswer {
                            content: brain_response.content,
                        });
                        return Ok((llm_response, "local-brain".to_string()));
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("LocalBrain failed: {}", scrub_text(&e.to_string()));
                    }
                    Err(_) => {
                        tracing::warn!("LocalBrain timed out after 120s");
                    }
                }
            } else {
                tracing::debug!("LocalBrain not available, falling back to providers");
            }
        }

        // If no providers available, return error immediately
        if self.providers.is_empty() {
            return Err(LLMError::ProviderUnavailable(
                "No LLM providers configured".to_string(),
            ));
        }

        // Analyze task and rank providers
        let profile = self.analyze_task(messages);
        let ranked_providers = self.rank_providers(&profile);
        let mut failures: Vec<String> = Vec::new();

        // Try each provider in order with timeout (Requirement 4.5)
        // Local providers (Ollama) get 120s for model loading + generation
        // Cloud providers get 30s (fast API responses)
        for provider in ranked_providers {
            if !provider.check_health().await {
                tracing::warn!(
                    "Skipping provider {} because health check failed",
                    provider.name()
                );
                failures.push(format!("{}: health check failed", provider.name()));
                continue;
            }

            let timeout_secs = if provider.is_local() { 120 } else { 30 };
            tracing::debug!(
                "Attempting provider: {} (timeout: {}s)",
                provider.name(),
                timeout_secs
            );

            let result = tokio::time::timeout(
                Duration::from_secs(timeout_secs),
                provider.generate(messages),
            )
            .await;

            match result {
                Ok(Ok(response)) => {
                    tracing::info!("Provider {} succeeded", provider.name());
                    return Ok((response, provider.name().to_string()));
                }
                Ok(Err(e)) => {
                    let scrubbed_error = scrub_text(&e.to_string());
                    tracing::warn!("Provider {} failed: {}", provider.name(), scrubbed_error);
                    let compact_error = {
                        let mut text = scrubbed_error.replace('\n', " ");
                        if text.len() > 200 {
                            text.truncate(200);
                            text.push_str("...");
                        }
                        text
                    };
                    failures.push(format!("{}: {}", provider.name(), compact_error));
                }
                Err(_) => {
                    tracing::warn!(
                        "Provider {} timed out after {}s",
                        provider.name(),
                        timeout_secs
                    );
                    failures.push(format!(
                        "{}: timed out after {}s",
                        provider.name(),
                        timeout_secs
                    ));
                }
            }
        }

        // All providers failed
        tracing::error!("All LLM providers exhausted");
        if failures.is_empty() {
            Err(LLMError::ProviderUnavailable(
                "All LLM providers failed".to_string(),
            ))
        } else {
            Err(LLMError::ProviderUnavailable(format!(
                "All LLM providers failed: {}",
                failures.join("; ")
            )))
        }
    }

    /// Check the health of all registered providers
    /// Returns a list of (provider_name, is_healthy)
    pub async fn check_health(&self) -> Vec<(&str, bool)> {
        let mut results = Vec::new();
        for provider in &self.providers {
            let is_healthy = provider.check_health().await;
            results.push((provider.name(), is_healthy));
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LLMError, LLMResponse};
    use async_trait::async_trait;

    // Mock provider for testing
    struct MockProvider {
        name: String,
        is_local: bool,
        cost_per_1k: f64,
    }

    impl MockProvider {
        fn new(name: &str, is_local: bool, cost_per_1k: f64) -> Self {
            Self {
                name: name.to_string(),
                is_local,
                cost_per_1k,
            }
        }
    }

    #[async_trait]
    impl LLMProvider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        fn is_local(&self) -> bool {
            self.is_local
        }

        fn estimated_cost(&self, tokens: usize) -> f64 {
            (tokens as f64 / 1000.0) * self.cost_per_1k
        }

        async fn generate(&self, _messages: &[Message]) -> Result<LLMResponse, LLMError> {
            unimplemented!("Mock provider doesn't implement generate")
        }
    }

    fn create_test_config() -> Arc<LLMConfig> {
        Arc::new(LLMConfig {
            default_provider: "ollama".to_string(),
            sensitivity_threshold: 0.7,
            complexity_threshold: 0.8,
            ollama: Default::default(),
            openai: Default::default(),
            anthropic: Default::default(),
            gemini: Default::default(),
            nvidia_nim: Default::default(),
            custom_providers: vec![],
        })
    }

    #[test]
    fn test_task_profile_creation() {
        let profile = TaskProfile::new(0.5, 0.8, 1000);
        assert_eq!(profile.sensitivity, 0.5);
        assert_eq!(profile.complexity, 0.8);
        assert_eq!(profile.estimated_tokens, 1000);
    }

    #[test]
    fn test_task_profile_clamping() {
        let profile = TaskProfile::new(1.5, -0.5, 1000);
        assert_eq!(profile.sensitivity, 1.0);
        assert_eq!(profile.complexity, 0.0);
    }

    #[test]
    fn test_calculate_sensitivity_no_keywords() {
        let config = create_test_config();
        let router = LLMRouter::new(vec![], config);

        let messages = vec![
            Message::user("Hello, how are you?"),
            Message::assistant("I'm doing well, thank you!"),
        ];

        let sensitivity = router.calculate_sensitivity(&messages);
        assert_eq!(sensitivity, 0.0);
    }

    #[test]
    fn test_calculate_sensitivity_with_keywords() {
        let config = create_test_config();
        let router = LLMRouter::new(vec![], config);

        let messages = vec![
            Message::user("I need to store my password and api_key"),
            Message::assistant("I can help with that"),
        ];

        let sensitivity = router.calculate_sensitivity(&messages);
        assert!(sensitivity > 0.0);
        assert!(sensitivity <= 1.0);
    }

    #[test]
    fn test_calculate_sensitivity_caps_at_one() {
        let config = create_test_config();
        let router = LLMRouter::new(vec![], config);

        let messages = vec![
            Message::user("password credential secret token api_key private_key ssh .env ssn credit_card bank account"),
        ];

        let sensitivity = router.calculate_sensitivity(&messages);
        assert_eq!(sensitivity, 1.0);
    }

    #[test]
    fn test_calculate_complexity_empty() {
        let config = create_test_config();
        let router = LLMRouter::new(vec![], config);

        let messages = vec![];
        let complexity = router.calculate_complexity(&messages);
        assert_eq!(complexity, 0.0);
    }

    #[test]
    fn test_calculate_complexity_simple() {
        let config = create_test_config();
        let router = LLMRouter::new(vec![], config);

        let messages = vec![Message::user("Hi"), Message::assistant("Hello")];

        let complexity = router.calculate_complexity(&messages);
        assert!(complexity < 0.5);
    }

    #[test]
    fn test_calculate_complexity_with_code() {
        let config = create_test_config();
        let router = LLMRouter::new(vec![], config);

        let messages = vec![Message::user("Here's my code:\n```rust\nfn main() {}\n```")];

        let complexity = router.calculate_complexity(&messages);
        assert!(complexity > 0.0);
    }

    #[test]
    fn test_estimate_tokens() {
        let config = create_test_config();
        let router = LLMRouter::new(vec![], config);

        let messages = vec![Message::user("This is a test message")];

        let tokens = router.estimate_tokens(&messages);
        // "This is a test message" = 23 chars / 4 = ~5-6 tokens
        assert!((5..=6).contains(&tokens));
    }

    #[test]
    fn test_analyze_task() {
        let config = create_test_config();
        let router = LLMRouter::new(vec![], config);

        let messages = vec![Message::user("I need help with my password")];

        let profile = router.analyze_task(&messages);
        assert!(profile.sensitivity > 0.0);
        assert!(profile.complexity >= 0.0);
        assert!(profile.estimated_tokens > 0);
    }

    #[test]
    fn test_rank_providers_prefer_local_for_sensitive() {
        let config = create_test_config();

        let providers: Vec<Box<dyn LLMProvider>> = vec![
            Box::new(MockProvider::new("openai", false, 0.002)),
            Box::new(MockProvider::new("ollama", true, 0.0)),
            Box::new(MockProvider::new("anthropic", false, 0.003)),
        ];

        let router = LLMRouter::new(providers, config);

        // High sensitivity task
        let profile = TaskProfile::new(0.9, 0.3, 1000);
        let ranked = router.rank_providers(&profile);

        // Local provider (ollama) should be first
        assert_eq!(ranked[0].name(), "ollama");
    }

    #[test]
    fn test_rank_providers_prefer_cloud_for_complex() {
        let config = create_test_config();

        let providers: Vec<Box<dyn LLMProvider>> = vec![
            Box::new(MockProvider::new("ollama", true, 0.0)),
            Box::new(MockProvider::new("openai", false, 0.002)),
            Box::new(MockProvider::new("anthropic", false, 0.003)),
        ];

        let router = LLMRouter::new(providers, config);

        // High complexity task
        let profile = TaskProfile::new(0.3, 0.9, 1000);
        let ranked = router.rank_providers(&profile);

        // Cloud provider should be first (not ollama)
        assert_ne!(ranked[0].name(), "ollama");
    }

    #[test]
    fn test_rank_providers_consider_cost() {
        let config = create_test_config();

        let providers: Vec<Box<dyn LLMProvider>> = vec![
            Box::new(MockProvider::new("expensive", false, 0.010)),
            Box::new(MockProvider::new("cheap", false, 0.001)),
            Box::new(MockProvider::new("medium", false, 0.005)),
        ];

        let router = LLMRouter::new(providers, config);

        // Low sensitivity, low complexity - cost should be main factor
        let profile = TaskProfile::new(0.3, 0.3, 1000);
        let ranked = router.rank_providers(&profile);

        // Cheaper provider should rank higher
        assert_eq!(ranked[0].name(), "cheap");
        assert_eq!(ranked[1].name(), "medium");
        assert_eq!(ranked[2].name(), "expensive");
    }

    #[test]
    fn test_rank_providers_balanced_task() {
        let config = create_test_config();

        let providers: Vec<Box<dyn LLMProvider>> = vec![
            Box::new(MockProvider::new("ollama", true, 0.0)),
            Box::new(MockProvider::new("openai", false, 0.002)),
        ];

        let router = LLMRouter::new(providers, config);

        // Balanced task (below thresholds)
        let profile = TaskProfile::new(0.5, 0.5, 1000);
        let ranked = router.rank_providers(&profile);

        // Should prefer cheaper option (ollama)
        assert_eq!(ranked[0].name(), "ollama");
    }
}
