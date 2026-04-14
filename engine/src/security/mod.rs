pub mod approvals;
pub mod command_executor;
pub mod crypto;
pub mod fs_guard;
pub mod injection_detector;
pub mod local_auth;
pub mod prompt_override;
pub mod rate_limiter;
pub mod risk_assessor;
pub mod secrets;

pub use approvals::ApprovalRequest;
pub use command_executor::CommandExecutor;
pub use crypto::CryptoModule;
pub use fs_guard::FileSystemGuard;
pub use injection_detector::InjectionDetector;
pub use local_auth::{
    can_reset_with_device_secret, configure_password_for_config, describe_protection_state,
    password_protection_state, password_protection_state_for, reset_password_for_config,
    verify_password, verify_recovery_code, PasswordProtectionState, PasswordSetupArtifacts,
};
pub use prompt_override::PromptOverrideDetector;
pub use rate_limiter::RateLimiter;
pub use risk_assessor::{Operation, OperationSource, RiskAssessor, RiskTier};
pub use secrets::{SecretCache, SecretManager, SecretString};
