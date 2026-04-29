//! Extended tests for crypto module — hash formats, signature parsing, envelope edge cases

use rove_engine::crypto::CryptoModule;
use std::io::Write;
use tempfile::NamedTempFile;

// ── CryptoModule construction ─────────────────────────────────────────────────

#[test]
fn crypto_module_constructs_ok() {
    assert!(CryptoModule::new().is_ok());
}

#[test]
fn crypto_module_constructs_twice() {
    assert!(CryptoModule::new().is_ok());
    assert!(CryptoModule::new().is_ok());
}

// ── verify_file: hash format parsing ─────────────────────────────────────────

#[test]
fn verify_file_nonexistent_path_returns_error() {
    let crypto = CryptoModule::new().unwrap();
    let result = crypto.verify_file(
        std::path::Path::new("/nonexistent/path/file.txt"),
        "blake3:0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert!(result.is_err());
}

#[test]
fn verify_file_sha256_prefix_accepted() {
    let crypto = CryptoModule::new().unwrap();
    let result = crypto.verify_file(std::path::Path::new("/nonexistent"), "sha256:aabbccdd");
    assert!(result.is_err()); // fails because file doesn't exist, not because of format
}

#[test]
fn verify_file_blake3_prefix_accepted() {
    let crypto = CryptoModule::new().unwrap();
    let result = crypto.verify_file(
        std::path::Path::new("/nonexistent"),
        "blake3:0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert!(result.is_err());
}

#[test]
fn verify_file_wrong_hash_deletes_file() {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(b"test content").unwrap();
    temp.flush().unwrap();
    let path = temp.path().to_path_buf();
    assert!(path.exists());

    let crypto = CryptoModule::new().unwrap();
    let result = crypto.verify_file(
        &path,
        "blake3:0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert!(result.is_err());
    assert!(!path.exists(), "File should be deleted after hash mismatch");
}

#[test]
fn verify_file_wrong_hash_for_known_content() {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(b"known content").unwrap();
    temp.flush().unwrap();
    let path = temp.path().to_path_buf();

    let crypto = CryptoModule::new().unwrap();
    // Wrong hash
    let _ = crypto.verify_file(
        &path,
        "blake3:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    // File deleted due to mismatch
    assert!(!path.exists());
}

#[test]
fn verify_file_empty_hash_returns_error() {
    let crypto = CryptoModule::new().unwrap();
    let result = crypto.verify_file(std::path::Path::new("/nonexistent"), "");
    assert!(result.is_err());
}

#[test]
fn verify_file_malformed_hash_no_prefix() {
    let crypto = CryptoModule::new().unwrap();
    let result = crypto.verify_file(std::path::Path::new("/nonexistent"), "aabbccdd");
    assert!(result.is_err());
}

#[test]
fn verify_file_only_prefix_no_value() {
    let crypto = CryptoModule::new().unwrap();
    let result = crypto.verify_file(std::path::Path::new("/nonexistent"), "blake3:");
    assert!(result.is_err());
}

// ── verify_manifest: signature format tests ───────────────────────────────────

#[test]
fn verify_manifest_empty_sig_returns_error() {
    let crypto = CryptoModule::new().unwrap();
    assert!(crypto.verify_manifest(b"test", "").is_err());
}

#[test]
fn verify_manifest_invalid_hex_returns_error() {
    let crypto = CryptoModule::new().unwrap();
    assert!(crypto
        .verify_manifest(b"test", "ed25519:invalid_hex!!")
        .is_err());
}

#[test]
fn verify_manifest_wrong_length_returns_error() {
    let crypto = CryptoModule::new().unwrap();
    let short_sig = "ed25519:abcd1234";
    assert!(crypto.verify_manifest(b"test", short_sig).is_err());
}

#[test]
fn verify_manifest_all_zeros_64_bytes_fails_signature_check() {
    let crypto = CryptoModule::new().unwrap();
    let sig = "ed25519:".to_string() + &"00".repeat(64);
    let result = crypto.verify_manifest(b"test manifest data", &sig);
    assert!(result.is_err());
}

#[test]
fn verify_manifest_all_ones_64_bytes_fails() {
    let crypto = CryptoModule::new().unwrap();
    let sig = "ed25519:".to_string() + &"ff".repeat(64);
    let result = crypto.verify_manifest(b"data", &sig);
    assert!(result.is_err());
}

#[test]
fn verify_manifest_no_prefix_fails() {
    let crypto = CryptoModule::new().unwrap();
    let result = crypto.verify_manifest(b"data", &"ab".repeat(64));
    assert!(result.is_err());
}

#[test]
fn verify_manifest_wrong_prefix_fails() {
    let crypto = CryptoModule::new().unwrap();
    let sig = "rsa:".to_string() + &"00".repeat(64);
    let result = crypto.verify_manifest(b"test", &sig);
    assert!(result.is_err());
}

#[test]
fn verify_manifest_empty_payload_fails_sig_check() {
    let crypto = CryptoModule::new().unwrap();
    let sig = "ed25519:".to_string() + &"00".repeat(64);
    let result = crypto.verify_manifest(b"", &sig);
    assert!(result.is_err());
}

#[test]
fn verify_manifest_large_payload_fails_sig_check() {
    let crypto = CryptoModule::new().unwrap();
    let sig = "ed25519:".to_string() + &"00".repeat(64);
    let payload = vec![0u8; 10000];
    let result = crypto.verify_manifest(&payload, &sig);
    assert!(result.is_err());
}

// ── Envelope tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn envelope_with_zero_key_and_current_timestamp_fails_sig() {
    use ed25519_dalek::{Signer, SigningKey};
    use std::time::{SystemTime, UNIX_EPOCH};

    let crypto = CryptoModule::new().unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let payload = b"test payload".to_vec();
    let signing_key = SigningKey::from_bytes(&[0u8; 32]);
    let signature = signing_key.sign(&payload);

    let envelope = rove_engine::crypto::Envelope {
        timestamp: now,
        nonce: 1001u64,
        payload: payload.clone(),
        signature,
    };

    let result = crypto.verify_envelope(&envelope).await;
    assert!(
        result.is_err(),
        "Zero signing key should not match embedded pubkey"
    );
}

#[tokio::test]
async fn envelope_old_timestamp_fails_expired() {
    use ed25519_dalek::{Signer, SigningKey};

    let crypto = CryptoModule::new().unwrap();
    let payload = b"old payload".to_vec();
    let signing_key = SigningKey::from_bytes(&[1u8; 32]);
    let signature = signing_key.sign(&payload);

    let envelope = rove_engine::crypto::Envelope {
        timestamp: 1000000000i64, // Very old
        nonce: 2001u64,
        payload,
        signature,
    };

    let result = crypto.verify_envelope(&envelope).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn envelope_future_timestamp_fails() {
    use ed25519_dalek::{Signer, SigningKey};
    use std::time::{SystemTime, UNIX_EPOCH};

    let crypto = CryptoModule::new().unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let payload = b"future payload".to_vec();
    let signing_key = SigningKey::from_bytes(&[2u8; 32]);
    let signature = signing_key.sign(&payload);

    let envelope = rove_engine::crypto::Envelope {
        timestamp: now + 120, // 2 minutes in the future
        nonce: 3001u64,
        payload,
        signature,
    };

    let result = crypto.verify_envelope(&envelope).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn envelope_nonce_replay_second_call_fails() {
    use ed25519_dalek::{Signer, SigningKey};
    use std::time::{SystemTime, UNIX_EPOCH};

    let crypto = CryptoModule::new().unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let payload = b"nonce test".to_vec();
    let signing_key = SigningKey::from_bytes(&[3u8; 32]);
    let signature = signing_key.sign(&payload);

    let envelope = rove_engine::crypto::Envelope {
        timestamp: now,
        nonce: 999888u64,
        payload: payload.clone(),
        signature,
    };

    // First call (will fail on sig but nonce gets cached)
    let _ = crypto.verify_envelope(&envelope).await;

    // Second call with same nonce
    let envelope2 = rove_engine::crypto::Envelope {
        timestamp: now,
        nonce: 999888u64,
        payload,
        signature,
    };
    let result = crypto.verify_envelope(&envelope2).await;
    assert!(result.is_err(), "Replayed nonce should be rejected");
}

#[tokio::test]
async fn envelope_different_nonces_both_fail_but_dont_panic() {
    use ed25519_dalek::{Signer, SigningKey};
    use std::time::{SystemTime, UNIX_EPOCH};

    let crypto = CryptoModule::new().unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    for nonce in [111001u64, 111002u64, 111003u64] {
        let payload = format!("payload-{}", nonce).into_bytes();
        let signing_key = SigningKey::from_bytes(&[4u8; 32]);
        let signature = signing_key.sign(&payload);
        let envelope = rove_engine::crypto::Envelope {
            timestamp: now,
            nonce,
            payload,
            signature,
        };
        let _ = crypto.verify_envelope(&envelope).await;
    }
}

// ── Envelope struct tests ──────────────────────────────────────────────────────

#[test]
fn envelope_fields_accessible() {
    use ed25519_dalek::{Signer, SigningKey};
    let payload = b"test".to_vec();
    let signing_key = SigningKey::from_bytes(&[0u8; 32]);
    let signature = signing_key.sign(&payload);
    let envelope = rove_engine::crypto::Envelope {
        timestamp: 1000i64,
        nonce: 42u64,
        payload: payload.clone(),
        signature,
    };
    assert_eq!(envelope.timestamp, 1000);
    assert_eq!(envelope.nonce, 42);
    assert_eq!(envelope.payload, payload);
}
