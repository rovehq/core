//! Cryptographic operations module
//!
//! This module provides cryptographic verification for the Rove engine:
//! - Ed25519 signature verification for manifests and core tools
//! - SHA-256 file hashing for integrity verification
//! - Automatic deletion of compromised files
//!
//! # Security
//!
//! The team public key is embedded at compile time via build.rs to prevent
//! tampering. All verification failures result in immediate file deletion
//! to prevent execution of compromised code.

use ed25519_dalek::{Signature, Verifier, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};
use sdk::errors::EngineError;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

// Keys are loaded from build.rs output in OUT_DIR.
// build.rs resolves each key from its own env var or manifest/ file.
//
// Two production keys are embedded, separately resolved:
//   - OFFICIAL  (ROVE_TEAM_OFFICIAL_PUBLIC_KEY / manifest/team_official_public_key.*):
//               engine releases, core-tools, official/reviewed plugins & drivers,
//               brains, revoked.json, community-manifest wrapper.
//   - COMMUNITY (ROVE_TEAM_COMMUNITY_PUBLIC_KEY / manifest/team_community_public_key.*):
//               community-tier plugin manifests only. Signed on PR merge by CI
//               holding the community private key as a secret.
//
// `verify_manifest_file` picks the key by trust_tier. Cross-signing
// (community key on an official manifest, or vice versa) is rejected.

#[cfg(not(feature = "production"))]
const TEAM_OFFICIAL_PUBLIC_KEY_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/dev_public_key.bin"));

#[cfg(feature = "production")]
const TEAM_OFFICIAL_PUBLIC_KEY_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/team_official_public_key.bin"));

#[cfg(not(feature = "production"))]
const TEAM_COMMUNITY_PUBLIC_KEY_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/dev_public_key.bin"));

#[cfg(feature = "production")]
const TEAM_COMMUNITY_PUBLIC_KEY_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/team_community_public_key.bin"));

/// Which key signed (or should sign) a given manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRole {
    /// Official + Reviewed tiers, engine releases, core-tools, drivers, brains.
    Official,
    /// Community-tier plugins only.
    Community,
}

/// Nonce cache window in seconds
///
/// Nonces are valid for 30 seconds to prevent replay attacks while allowing
/// for reasonable clock skew between systems.
const NONCE_WINDOW_SECS: u64 = 30;

/// Envelope for secure message transmission
///
/// An envelope contains a message payload along with cryptographic metadata
/// for verification: timestamp, nonce, and signature. This prevents replay
/// attacks and ensures message authenticity.
///
/// # Security
///
/// - Timestamp must be within 30 seconds of current time
/// - Nonce must not have been seen before (replay prevention)
/// - Signature must be valid for the payload
#[derive(Debug, Clone)]
pub struct Envelope {
    /// Unix timestamp when the envelope was created
    pub timestamp: i64,
    /// Unique nonce for replay prevention
    pub nonce: u64,
    /// Message payload
    pub payload: Vec<u8>,
    /// Ed25519 signature over the payload
    pub signature: Signature,
}

/// Nonce cache for replay prevention
///
/// Maintains a cache of recently seen nonces with their timestamps.
/// Nonces older than 30 seconds are automatically evicted.
///
/// # Thread Safety
///
/// This struct is thread-safe and can be shared across threads using Arc.
struct NonceCache {
    /// Map of nonce to timestamp when it was seen
    cache: HashMap<u64, u64>,
}

impl NonceCache {
    /// Create a new empty nonce cache
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Check if a nonce exists in the cache
    fn contains(&self, nonce: &u64) -> bool {
        self.cache.contains_key(nonce)
    }

    /// Insert a nonce into the cache with its timestamp
    fn insert(&mut self, nonce: u64, timestamp: u64) {
        self.cache.insert(nonce, timestamp);
    }

    /// Evict nonces older than the specified cutoff timestamp
    fn evict_older_than(&mut self, cutoff: u64) {
        self.cache.retain(|_, &mut ts| ts >= cutoff);
    }
}

/// Cryptographic operations module
///
/// Provides methods for:
/// - Verifying Ed25519 signatures on manifests
/// - Computing and verifying SHA-256 file hashes
/// - Deleting compromised files on verification failure
/// - Verifying envelopes with nonce-based replay prevention
pub struct CryptoModule {
    team_public_key: VerifyingKey,
    community_public_key: VerifyingKey,
    nonce_cache: Arc<Mutex<NonceCache>>,
}

impl CryptoModule {
    /// Create a new CryptoModule with the embedded team public keys
    ///
    /// # Errors
    ///
    /// Returns an error if either embedded public key is invalid or corrupted.
    /// This should never happen in a properly built binary.
    pub fn new() -> Result<Self, EngineError> {
        let team_public_key =
            Self::parse_embedded_key(TEAM_OFFICIAL_PUBLIC_KEY_BYTES, "official team public key")?;
        let community_public_key =
            Self::parse_embedded_key(TEAM_COMMUNITY_PUBLIC_KEY_BYTES, "community team public key")?;

        tracing::info!("CryptoModule initialized with official + community public keys");

        Ok(Self {
            team_public_key,
            community_public_key,
            nonce_cache: Arc::new(Mutex::new(NonceCache::new())),
        })
    }

    fn parse_embedded_key(bytes: &[u8], label: &str) -> Result<VerifyingKey, EngineError> {
        if bytes.len() != PUBLIC_KEY_LENGTH {
            return Err(EngineError::Config(format!(
                "Invalid {} length: expected {}, got {}",
                label,
                PUBLIC_KEY_LENGTH,
                bytes.len()
            )));
        }
        let key_bytes: [u8; PUBLIC_KEY_LENGTH] = bytes
            .try_into()
            .map_err(|_| EngineError::Config(format!("{} must be 32 bytes", label)))?;
        VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| EngineError::Config(format!("Invalid {}: {}", label, e)))
    }

    /// Create a CryptoModule with a specific verifying key (for testing).
    /// Both official and community roles resolve to the same key.
    #[cfg(test)]
    pub fn with_key(key: VerifyingKey) -> Self {
        Self {
            team_public_key: key,
            community_public_key: key,
            nonce_cache: Arc::new(Mutex::new(NonceCache::new())),
        }
    }

    /// Create a CryptoModule with distinct official + community keys (for testing).
    #[cfg(test)]
    pub fn with_keys(official: VerifyingKey, community: VerifyingKey) -> Self {
        Self {
            team_public_key: official,
            community_public_key: community,
            nonce_cache: Arc::new(Mutex::new(NonceCache::new())),
        }
    }

    fn key_for(&self, role: KeyRole) -> &VerifyingKey {
        match role {
            KeyRole::Official => &self.team_public_key,
            KeyRole::Community => &self.community_public_key,
        }
    }

    /// Whether we're running a production build
    pub fn is_production() -> bool {
        cfg!(feature = "production")
    }

    /// Verify a manifest signature using the OFFICIAL team public key.
    ///
    /// Kept for backwards compatibility — callers that know they are verifying
    /// an official-tier manifest (engine release, core-tool, etc.) can use this
    /// directly. For tier-driven routing, prefer `verify_manifest_file` or
    /// `verify_manifest_with_role`.
    pub fn verify_manifest(
        &self,
        manifest_bytes: &[u8],
        signature_hex: &str,
    ) -> Result<(), EngineError> {
        self.verify_manifest_with_role(manifest_bytes, signature_hex, KeyRole::Official)
    }

    /// Verify a manifest signature against the key for the requested role.
    pub fn verify_manifest_with_role(
        &self,
        manifest_bytes: &[u8],
        signature_hex: &str,
        role: KeyRole,
    ) -> Result<(), EngineError> {
        tracing::debug!(?role, "Verifying manifest signature");

        let signature = self.parse_signature(signature_hex)?;
        self.key_for(role)
            .verify(manifest_bytes, &signature)
            .map_err(|e| {
                tracing::error!(?role, "Manifest signature verification failed: {}", e);
                EngineError::InvalidSignature
            })?;

        tracing::info!(?role, "Manifest signature verified successfully");
        Ok(())
    }

    /// Verify a file's SHA-256 hash and delete it if verification fails
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to verify
    /// * `expected_hash` - Expected SHA-256 hash as hex string (no prefix)
    ///
    /// # Security
    ///
    /// **CRITICAL**: This method deletes the file on hash mismatch to prevent
    /// execution of tampered binaries.
    pub fn verify_file(&self, path: &Path, expected_hash: &str) -> Result<(), EngineError> {
        tracing::debug!("Verifying file hash: {}", path.display());

        // Parse expected hash (strip optional prefix, accept raw hex)
        let expected = self.parse_hash(expected_hash)?;

        // Compute SHA-256 hash of file
        let computed = self.compute_file_hash(path)?;

        // Compare hashes
        if computed != expected {
            tracing::error!(
                "Hash mismatch for {}: expected {}, got {}",
                path.display(),
                expected,
                computed
            );

            // Delete compromised file
            if let Err(e) = std::fs::remove_file(path) {
                tracing::error!(
                    "Failed to delete compromised file {}: {}",
                    path.display(),
                    e
                );
                return Err(EngineError::Io(e));
            }

            tracing::warn!("Deleted compromised file: {}", path.display());
            return Err(EngineError::HashMismatch(path.display().to_string()));
        }

        tracing::debug!("File hash verified: {}", path.display());
        Ok(())
    }

    /// Verify an individual tool's Ed25519 signature
    ///
    /// Computes the SHA-256 hash of the file and verifies the signature
    /// against that hash string.
    pub fn verify_file_signature(
        &self,
        path: &Path,
        signature_hex: &str,
    ) -> Result<(), EngineError> {
        tracing::debug!("Verifying file signature: {}", path.display());

        if signature_hex.contains("PLACEHOLDER") || signature_hex.contains("LOCAL_DEV") {
            if Self::is_production() {
                return Err(EngineError::InvalidSignature);
            }
            tracing::debug!(
                "Accepting dev placeholder file signature for {} (non-production build)",
                path.display()
            );
            return Ok(());
        }

        // Compute file hash
        let file_hash = self.compute_file_hash(path)?;

        // Parse signature
        let signature = self.parse_signature(signature_hex)?;

        // File signatures (core-tool dylibs, driver binaries, etc.) always
        // verify under the Official key. Community artifacts are WASM and
        // signed at the manifest level, not per-file.
        self.team_public_key
            .verify(file_hash.as_bytes(), &signature)
            .map_err(|e| {
                tracing::error!(
                    "File signature verification failed for {}: {}",
                    path.display(),
                    e
                );
                EngineError::InvalidSignature
            })?;

        tracing::info!("File signature verified: {}", path.display());
        Ok(())
    }

    /// Verify an envelope with timestamp, nonce, and signature checks
    ///
    /// Protocol:
    /// 1. Check timestamp is within 30 seconds
    /// 2. Check nonce is not in cache (replay prevention)
    /// 3. Verify Ed25519 signature
    /// 4. Insert nonce into cache
    /// 5. Evict old nonces
    pub async fn verify_envelope(&self, envelope: &Envelope) -> Result<Vec<u8>, EngineError> {
        tracing::debug!("Verifying envelope with nonce {}", envelope.nonce);

        // Get current timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| EngineError::Config(format!("System time error: {}", e)))?
            .as_secs();

        // Check timestamp is within 30 seconds
        let time_diff = (now as i64 - envelope.timestamp).abs();
        if time_diff > NONCE_WINDOW_SECS as i64 {
            tracing::warn!(
                "Envelope timestamp outside valid window: {} seconds difference",
                time_diff
            );
            return Err(EngineError::EnvelopeExpired);
        }

        // Check nonce is not in cache (replay prevention)
        let mut cache = self.nonce_cache.lock().await;
        if cache.contains(&envelope.nonce) {
            tracing::error!(
                "Nonce {} has been used before (replay attack detected)",
                envelope.nonce
            );
            return Err(EngineError::NonceReused);
        }

        // Verify Ed25519 signature
        self.team_public_key
            .verify(&envelope.payload, &envelope.signature)
            .map_err(|e| {
                tracing::error!("Envelope signature verification failed: {}", e);
                EngineError::InvalidSignature
            })?;

        // Insert nonce into cache before processing
        cache.insert(envelope.nonce, now);
        tracing::debug!("Nonce {} inserted into cache", envelope.nonce);

        // Evict nonces older than 30 seconds
        let cutoff = now.saturating_sub(NONCE_WINDOW_SECS);
        cache.evict_older_than(cutoff);

        tracing::info!("Envelope verified successfully");
        Ok(envelope.payload.clone())
    }

    /// Compute SHA-256 hash of a file
    ///
    /// Returns the hex-encoded SHA-256 hash.
    fn compute_file_hash(&self, path: &Path) -> Result<String, EngineError> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();

        let mut buffer = [0u8; 8192];
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let hash = hasher.finalize();
        Ok(hex::encode(hash))
    }

    /// Compute SHA-256 hash of raw bytes
    pub fn compute_hash(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    }

    /// Compute BLAKE3 hash of raw bytes (hex-encoded)
    pub fn compute_blake3(data: &[u8]) -> String {
        hex::encode(blake3::hash(data).as_bytes())
    }

    /// Parse a hash string, accepting raw hex or prefixed formats
    ///
    /// Accepts:
    /// - Raw hex: "abcd1234..."
    /// - Prefixed: "sha256:abcd1234..."
    /// - Legacy: "blake3:abcd1234..." (prefix stripped, treated as raw hex)
    fn parse_hash(&self, hash_str: &str) -> Result<String, EngineError> {
        if let Some(hex) = hash_str.strip_prefix("sha256:") {
            Ok(hex.to_string())
        } else if let Some(hex) = hash_str.strip_prefix("blake3:") {
            // Legacy compatibility — strip prefix, treat as hex
            Ok(hex.to_string())
        } else {
            // Accept raw hex (no prefix) — this is the standard format
            // Validate it looks like hex
            if hash_str.len() == 64 && hash_str.chars().all(|c| c.is_ascii_hexdigit()) {
                Ok(hash_str.to_string())
            } else if hash_str.is_empty() {
                Err(EngineError::Config("Empty hash string".to_string()))
            } else {
                // Accept any hex string (may be different length for other algorithms)
                Ok(hash_str.to_string())
            }
        }
    }

    /// Parse a signature string
    ///
    /// Accepts:
    /// - "ed25519:hex_string"
    /// - Raw hex string
    fn parse_signature(&self, sig_str: &str) -> Result<Signature, EngineError> {
        // Remove "ed25519:" prefix if present
        let hex = sig_str.strip_prefix("ed25519:").unwrap_or(sig_str);

        // Decode hex to bytes
        let bytes = hex::decode(hex).map_err(|e| {
            tracing::error!("Failed to decode signature hex: {}", e);
            EngineError::InvalidSignature
        })?;

        // Validate signature length
        if bytes.len() != SIGNATURE_LENGTH {
            tracing::error!(
                "Invalid signature length: expected {}, got {}",
                SIGNATURE_LENGTH,
                bytes.len()
            );
            return Err(EngineError::InvalidSignature);
        }

        let sig_bytes: [u8; SIGNATURE_LENGTH] = bytes
            .try_into()
            .map_err(|_| EngineError::InvalidSignature)?;

        Ok(Signature::from_bytes(&sig_bytes))
    }

    /// Canonicalize a JSON manifest for signing/verification
    ///
    /// Strips `signature` and `signed_at` fields, then serializes
    /// as compact JSON with sorted keys (BTreeMap ordering from serde_json::Value).
    ///
    /// Both Python signer and Rust verifier must produce identical bytes:
    /// - Python: `json.dumps(data, sort_keys=True, separators=(',', ':'))`
    /// - Rust: `serde_json::to_string()` on Value (BTreeMap = sorted keys, compact)
    pub fn canonicalize_manifest(manifest_json: &[u8]) -> Result<Vec<u8>, EngineError> {
        let mut value: serde_json::Value = serde_json::from_slice(manifest_json)
            .map_err(|e| EngineError::Config(format!("Invalid manifest JSON: {}", e)))?;

        // Remove signature-related fields before canonical serialization
        if let Some(obj) = value.as_object_mut() {
            obj.remove("signature");
            obj.remove("signed_at");
        }

        // serde_json::Value uses BTreeMap internally, so keys are already sorted alphabetically.
        // to_string() produces compact JSON with no whitespace — matching Python's
        // json.dumps(data, sort_keys=True, separators=(',', ':'))
        let canonical = serde_json::to_string(&value)
            .map_err(|e| EngineError::Config(format!("Failed to serialize manifest: {}", e)))?;

        Ok(canonical.into_bytes())
    }

    /// Verify a manifest file, routing to the correct key by trust tier.
    ///
    /// Flow:
    /// 1. Parse JSON, extract `signature` and `trust_tier`
    /// 2. Pick key role: Community tier ⇒ community key, everything else ⇒ official
    /// 3. Strip signature fields, canonicalize
    /// 4. Verify canonical bytes against the role's key
    ///
    /// Cross-signing (community key on a non-community tier, or vice versa)
    /// fails at signature verification — the wrong key cannot produce a valid
    /// signature for the canonical bytes.
    pub fn verify_manifest_file(&self, manifest_json: &[u8]) -> Result<(), EngineError> {
        let value: serde_json::Value = serde_json::from_slice(manifest_json)
            .map_err(|e| EngineError::Config(format!("Invalid manifest JSON: {}", e)))?;

        let signature = value
            .get("signature")
            .and_then(|s| s.as_str())
            .ok_or_else(|| EngineError::Config("No signature in manifest".to_string()))?;

        if signature.contains("PLACEHOLDER") || signature.contains("LOCAL_DEV") {
            if Self::is_production() {
                return Err(EngineError::InvalidSignature);
            }
            tracing::debug!("Accepting dev placeholder signature (non-production build)");
            return Ok(());
        }

        let role = role_for_manifest(&value);
        let canonical = Self::canonicalize_manifest(manifest_json)?;
        self.verify_manifest_with_role(&canonical, signature, role)
    }
}

/// Pick the signing key role based on the manifest's `trust_tier` field.
///
/// Accepts either string form ("Official" / "Reviewed" / "Community") or
/// integer form (0, 1, 2) — both appear in manifests today. Unknown or
/// missing tiers default to Official, matching the previous single-key
/// behavior.
fn role_for_manifest(value: &serde_json::Value) -> KeyRole {
    let tier = value.get("trust_tier");
    match tier {
        Some(serde_json::Value::String(s)) => {
            if s.eq_ignore_ascii_case("community") {
                KeyRole::Community
            } else {
                KeyRole::Official
            }
        }
        Some(serde_json::Value::Number(n)) => {
            if n.as_i64() == Some(2) {
                KeyRole::Community
            } else {
                KeyRole::Official
            }
        }
        _ => KeyRole::Official,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Generate a test keypair and return (signing_key, crypto_module)
    fn test_crypto() -> (SigningKey, CryptoModule) {
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let verifying_key = signing_key.verifying_key();
        let crypto = CryptoModule::with_key(verifying_key);
        (signing_key, crypto)
    }

    #[test]
    fn test_compute_file_hash() {
        let (_, crypto) = test_crypto();

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"test content").unwrap();
        temp_file.flush().unwrap();

        let hash = crypto.compute_file_hash(temp_file.path()).unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA-256 hex = 64 chars

        // Verify it's a valid SHA-256 hash of "test content"
        let expected = CryptoModule::compute_hash(b"test content");
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let h1 = CryptoModule::compute_hash(b"hello world");
        let h2 = CryptoModule::compute_hash(b"hello world");
        assert_eq!(h1, h2);

        let h3 = CryptoModule::compute_hash(b"different");
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_verify_file_hash_match() {
        let (_, crypto) = test_crypto();

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"test content").unwrap();
        temp_file.flush().unwrap();

        let expected = CryptoModule::compute_hash(b"test content");
        let result = crypto.verify_file(temp_file.path(), &expected);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_file_hash_mismatch_deletes_file() {
        let (_, crypto) = test_crypto();

        let mut temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();
        temp_file.write_all(b"test content").unwrap();
        temp_file.flush().unwrap();

        // Keep file alive by extracting the path before dropping
        let _ = temp_file.into_temp_path();

        let result = crypto.verify_file(
            &path,
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(result.is_err());
        // File should be deleted
        assert!(!path.exists());
    }

    #[test]
    fn test_parse_hash_raw_hex() {
        let (_, crypto) = test_crypto();

        // Raw hex (no prefix) — standard format from build-manifest.py
        let hash = crypto
            .parse_hash("c9508e28452d11f76561c45c0bbb0b517161012269f286823c6aad553c0a780f")
            .unwrap();
        assert_eq!(
            hash,
            "c9508e28452d11f76561c45c0bbb0b517161012269f286823c6aad553c0a780f"
        );
    }

    #[test]
    fn test_parse_hash_sha256_prefix() {
        let (_, crypto) = test_crypto();

        let hash = crypto
            .parse_hash("sha256:abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234")
            .unwrap();
        assert_eq!(
            hash,
            "abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234"
        );
    }

    #[test]
    fn test_parse_hash_blake3_legacy_prefix() {
        let (_, crypto) = test_crypto();

        let hash = crypto.parse_hash("blake3:abcd1234").unwrap();
        assert_eq!(hash, "abcd1234");
    }

    #[test]
    fn test_parse_hash_empty_fails() {
        let (_, crypto) = test_crypto();
        let result = crypto.parse_hash("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_signature_with_prefix() {
        let (_, crypto) = test_crypto();

        let sig_hex = "ed25519:".to_string() + &"ab".repeat(64);
        let result = crypto.parse_signature(&sig_hex);
        // Should parse (may or may not be a valid signature, but parsing succeeds)
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_signature_without_prefix() {
        let (_, crypto) = test_crypto();

        let sig_hex = "ab".repeat(64);
        let result = crypto.parse_signature(&sig_hex);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_file_signature_accepts_local_dev_placeholder_in_dev() {
        let (_, crypto) = test_crypto();

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"dev payload").unwrap();
        temp_file.flush().unwrap();

        let result = crypto.verify_file_signature(temp_file.path(), "LOCAL_DEV_PAYLOAD_SIGNATURE");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_signature_invalid_hex() {
        let (_, crypto) = test_crypto();
        let result = crypto.parse_signature("not_valid_hex");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_signature_wrong_length() {
        let (_, crypto) = test_crypto();
        let result = crypto.parse_signature("abcd");
        assert!(result.is_err());
    }

    #[test]
    fn test_manifest_sign_and_verify() {
        use ed25519_dalek::Signer;

        let (signing_key, crypto) = test_crypto();

        let manifest = serde_json::json!({
            "version": "1.0.0",
            "plugins": [],
            "core_tools": [],
            "signature": "will_be_removed",
            "signed_at": "will_be_removed"
        });
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();

        // Canonicalize
        let canonical = CryptoModule::canonicalize_manifest(&manifest_bytes).unwrap();

        // Sign
        let signature = signing_key.sign(&canonical);
        let sig_hex = hex::encode(signature.to_bytes());

        // Verify
        let result = crypto.verify_manifest(&canonical, &sig_hex);
        assert!(result.is_ok());
    }

    #[test]
    fn test_manifest_tampered_fails() {
        use ed25519_dalek::Signer;

        let (signing_key, crypto) = test_crypto();

        let manifest = serde_json::json!({
            "version": "1.0.0",
            "plugins": []
        });
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let canonical = CryptoModule::canonicalize_manifest(&manifest_bytes).unwrap();

        // Sign original
        let signature = signing_key.sign(&canonical);
        let sig_hex = hex::encode(signature.to_bytes());

        // Tamper with manifest
        let tampered = serde_json::json!({
            "version": "1.0.0",
            "plugins": [{"id": "malware", "hash": "evil"}]
        });
        let tampered_bytes = serde_json::to_vec(&tampered).unwrap();
        let tampered_canonical = CryptoModule::canonicalize_manifest(&tampered_bytes).unwrap();

        // Verification should fail
        let result = crypto.verify_manifest(&tampered_canonical, &sig_hex);
        assert!(result.is_err());
    }

    #[test]
    fn test_canonicalize_strips_signature_fields() {
        let manifest = serde_json::json!({
            "version": "1.0.0",
            "plugins": [],
            "signature": "some_sig",
            "signed_at": "some_time"
        });
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let canonical = CryptoModule::canonicalize_manifest(&bytes).unwrap();
        let canonical_str = String::from_utf8(canonical).unwrap();

        assert!(!canonical_str.contains("signature"));
        assert!(!canonical_str.contains("signed_at"));
        assert!(canonical_str.contains("version"));
        assert!(canonical_str.contains("plugins"));
    }

    #[test]
    fn test_canonicalize_sorted_keys() {
        // Keys should be sorted alphabetically (BTreeMap)
        let manifest = serde_json::json!({
            "zebra": 1,
            "alpha": 2,
            "middle": 3
        });
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let canonical = CryptoModule::canonicalize_manifest(&bytes).unwrap();
        let canonical_str = String::from_utf8(canonical).unwrap();

        // Should be: {"alpha":2,"middle":3,"zebra":1}
        assert_eq!(canonical_str, r#"{"alpha":2,"middle":3,"zebra":1}"#);
    }

    #[test]
    fn test_canonicalize_compact_no_whitespace() {
        let manifest = serde_json::json!({
            "key": "value",
            "num": 42
        });
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let canonical = CryptoModule::canonicalize_manifest(&bytes).unwrap();
        let canonical_str = String::from_utf8(canonical).unwrap();

        // No spaces, no newlines
        assert!(!canonical_str.contains(' '));
        assert!(!canonical_str.contains('\n'));
    }

    #[test]
    fn test_verify_file_signature() {
        use ed25519_dalek::Signer;

        let (signing_key, crypto) = test_crypto();

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"plugin binary data").unwrap();
        temp_file.flush().unwrap();

        // Compute hash and sign it
        let hash = crypto.compute_file_hash(temp_file.path()).unwrap();
        let signature = signing_key.sign(hash.as_bytes());
        let sig_hex = hex::encode(signature.to_bytes());

        // Verify
        let result = crypto.verify_file_signature(temp_file.path(), &sig_hex);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_manifest_file_placeholder_dev() {
        let (_, crypto) = test_crypto();

        let manifest = serde_json::json!({
            "version": "1.0.0",
            "plugins": [],
            "signature": "LOCAL_DEV_PLACEHOLDER_SIGNATURE",
            "signed_at": "local-development"
        });
        let bytes = serde_json::to_vec(&manifest).unwrap();

        // In non-production builds, placeholder should be accepted
        if !CryptoModule::is_production() {
            let result = crypto.verify_manifest_file(&bytes);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_verify_manifest_file_real_signature() {
        use ed25519_dalek::Signer;

        let (signing_key, crypto) = test_crypto();

        // Build manifest without signature
        let manifest_data = serde_json::json!({
            "version": "1.0.0",
            "plugins": [],
            "core_tools": []
        });
        let data_bytes = serde_json::to_vec(&manifest_data).unwrap();
        let canonical = CryptoModule::canonicalize_manifest(&data_bytes).unwrap();

        // Sign canonical bytes
        let signature = signing_key.sign(&canonical);
        let sig_hex = hex::encode(signature.to_bytes());

        // Build full manifest with signature
        let full_manifest = serde_json::json!({
            "version": "1.0.0",
            "plugins": [],
            "core_tools": [],
            "signature": sig_hex,
            "signed_at": "2025-01-01T00:00:00Z"
        });
        let full_bytes = serde_json::to_vec(&full_manifest).unwrap();

        // Verify should succeed
        let result = crypto.verify_manifest_file(&full_bytes);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_envelope_valid() {
        use ed25519_dalek::Signer;

        let (signing_key, crypto) = test_crypto();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let payload = b"test message".to_vec();
        let signature = signing_key.sign(&payload);

        let envelope = Envelope {
            timestamp: now as i64,
            nonce: 12345,
            payload: payload.clone(),
            signature,
        };

        let result = crypto.verify_envelope(&envelope).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), payload);
    }

    #[tokio::test]
    async fn test_envelope_expired() {
        use ed25519_dalek::Signer;

        let (signing_key, crypto) = test_crypto();

        let payload = b"test".to_vec();
        let signature = signing_key.sign(&payload);

        let envelope = Envelope {
            timestamp: 1000, // way in the past
            nonce: 1,
            payload,
            signature,
        };

        let result = crypto.verify_envelope(&envelope).await;
        assert!(matches!(result, Err(EngineError::EnvelopeExpired)));
    }

    #[tokio::test]
    async fn test_envelope_nonce_replay() {
        use ed25519_dalek::Signer;

        let (signing_key, crypto) = test_crypto();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let payload = b"test".to_vec();
        let signature = signing_key.sign(&payload);

        let envelope = Envelope {
            timestamp: now as i64,
            nonce: 999,
            payload,
            signature,
        };

        // First should succeed
        assert!(crypto.verify_envelope(&envelope).await.is_ok());

        // Replay should fail
        let result = crypto.verify_envelope(&envelope).await;
        assert!(matches!(result, Err(EngineError::NonceReused)));
    }

    /// Dual-key routing: community manifest signed by the community key passes;
    /// community manifest signed by the official key (cross-signing) fails.
    #[test]
    fn test_verify_manifest_file_routes_by_trust_tier() {
        use ed25519_dalek::Signer;

        let official_sk = SigningKey::from_bytes(&[7u8; 32]);
        let community_sk = SigningKey::from_bytes(&[9u8; 32]);
        let crypto =
            CryptoModule::with_keys(official_sk.verifying_key(), community_sk.verifying_key());

        let community_manifest = serde_json::json!({
            "name": "echo-plugin",
            "version": "0.1.0",
            "trust_tier": "Community",
            "signature": ""
        });
        let bytes = serde_json::to_vec(&community_manifest).unwrap();
        let canonical = CryptoModule::canonicalize_manifest(&bytes).unwrap();

        // Signed with community key → passes
        let good_sig = hex::encode(community_sk.sign(&canonical).to_bytes());
        let mut signed = community_manifest.clone();
        signed["signature"] = serde_json::Value::String(good_sig);
        assert!(crypto
            .verify_manifest_file(&serde_json::to_vec(&signed).unwrap())
            .is_ok());

        // Signed with official key (cross-signing) → rejected
        let bad_sig = hex::encode(official_sk.sign(&canonical).to_bytes());
        let mut signed_wrong = community_manifest.clone();
        signed_wrong["signature"] = serde_json::Value::String(bad_sig);
        assert!(matches!(
            crypto.verify_manifest_file(&serde_json::to_vec(&signed_wrong).unwrap()),
            Err(EngineError::InvalidSignature)
        ));
    }

    /// Official manifest signed by community key is rejected (reverse cross-signing).
    #[test]
    fn test_official_manifest_rejects_community_signature() {
        use ed25519_dalek::Signer;

        let official_sk = SigningKey::from_bytes(&[7u8; 32]);
        let community_sk = SigningKey::from_bytes(&[9u8; 32]);
        let crypto =
            CryptoModule::with_keys(official_sk.verifying_key(), community_sk.verifying_key());

        let official_manifest = serde_json::json!({
            "name": "telegram",
            "version": "0.1.0",
            "trust_tier": "Official",
            "signature": ""
        });
        let bytes = serde_json::to_vec(&official_manifest).unwrap();
        let canonical = CryptoModule::canonicalize_manifest(&bytes).unwrap();

        // Community-signed official manifest → rejected
        let bad_sig = hex::encode(community_sk.sign(&canonical).to_bytes());
        let mut signed_wrong = official_manifest.clone();
        signed_wrong["signature"] = serde_json::Value::String(bad_sig);
        assert!(matches!(
            crypto.verify_manifest_file(&serde_json::to_vec(&signed_wrong).unwrap()),
            Err(EngineError::InvalidSignature)
        ));

        // Official-signed official manifest → passes
        let good_sig = hex::encode(official_sk.sign(&canonical).to_bytes());
        let mut signed = official_manifest.clone();
        signed["signature"] = serde_json::Value::String(good_sig);
        assert!(crypto
            .verify_manifest_file(&serde_json::to_vec(&signed).unwrap())
            .is_ok());
    }

    #[test]
    fn test_role_for_manifest_parses_tier() {
        assert_eq!(
            role_for_manifest(&serde_json::json!({"trust_tier": "Community"})),
            KeyRole::Community
        );
        assert_eq!(
            role_for_manifest(&serde_json::json!({"trust_tier": "community"})),
            KeyRole::Community
        );
        assert_eq!(
            role_for_manifest(&serde_json::json!({"trust_tier": 2})),
            KeyRole::Community
        );
        assert_eq!(
            role_for_manifest(&serde_json::json!({"trust_tier": "Official"})),
            KeyRole::Official
        );
        assert_eq!(
            role_for_manifest(&serde_json::json!({"trust_tier": "Reviewed"})),
            KeyRole::Official
        );
        assert_eq!(
            role_for_manifest(&serde_json::json!({"trust_tier": 0})),
            KeyRole::Official
        );
        assert_eq!(role_for_manifest(&serde_json::json!({})), KeyRole::Official);
    }
}
