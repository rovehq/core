pub struct SetupResult {
    pub workspace: String,
    pub provider_name: String,
    pub protocol: String,
    pub base_url: String,
    pub model: String,
    pub secret_key: String,
    pub api_key: String,
    pub max_risk_tier: u8,
    pub skipped_model: bool,
    pub daemon_password: String,
    pub recovery_code: Option<String>,
    pub auth_protection: Option<String>,
}
