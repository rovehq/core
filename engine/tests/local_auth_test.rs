//! Tests for security::local_auth — password hashing, verification, protection state

use rove_engine::security::local_auth::{
    describe_protection_state, hash_password, verify_password, PasswordProtectionState,
};

// ── hash_password tests ────────────────────────────────────────────────────────

#[test]
fn hash_password_returns_argon2_hash() {
    let hash = hash_password("correct horse battery").unwrap();
    assert!(hash.starts_with("$argon2"));
}

#[test]
fn hash_password_min_8_chars_ok() {
    let result = hash_password("12345678");
    assert!(result.is_ok());
}

#[test]
fn hash_password_7_chars_rejected() {
    let result = hash_password("1234567");
    assert!(result.is_err());
}

#[test]
fn hash_password_empty_rejected() {
    let result = hash_password("");
    assert!(result.is_err());
}

#[test]
fn hash_password_whitespace_only_rejected() {
    let result = hash_password("        "); // 8 spaces
    assert!(result.is_err());
}

#[test]
fn hash_password_leading_whitespace_trimmed() {
    // "  pass" = 2+4 = 6 chars after trim → rejected
    let result = hash_password("  pass  ");
    assert!(result.is_err());
}

#[test]
fn hash_password_different_calls_produce_different_hashes() {
    let h1 = hash_password("same-password-123").unwrap();
    let h2 = hash_password("same-password-123").unwrap();
    assert_ne!(h1, h2, "Argon2 should use different salts");
}

#[test]
fn hash_password_long_password_ok() {
    let long_pw = "a".repeat(100);
    let result = hash_password(&long_pw);
    assert!(result.is_ok());
}

#[test]
fn hash_password_with_special_chars() {
    let result = hash_password("p@ss!word#2024");
    assert!(result.is_ok());
}

#[test]
fn hash_password_unicode_ok() {
    let result = hash_password("パスワード1234");
    assert!(result.is_ok());
}

// ── verify_password tests ──────────────────────────────────────────────────────

#[test]
fn verify_password_correct_returns_true() {
    let hash = hash_password("correct horse battery staple").unwrap();
    let result = verify_password("correct horse battery staple", &hash).unwrap();
    assert!(result);
}

#[test]
fn verify_password_wrong_returns_false() {
    let hash = hash_password("correct horse battery staple").unwrap();
    let result = verify_password("wrong password here", &hash).unwrap();
    assert!(!result);
}

#[test]
fn verify_password_empty_against_hash_returns_false() {
    let hash = hash_password("my good password").unwrap();
    let result = verify_password("", &hash).unwrap();
    assert!(!result);
}

#[test]
fn verify_password_case_sensitive() {
    let hash = hash_password("Password123").unwrap();
    let result = verify_password("password123", &hash).unwrap();
    assert!(!result);
}

#[test]
fn verify_password_invalid_hash_returns_error() {
    let result = verify_password("some-password", "not-a-valid-hash");
    assert!(result.is_err());
}

#[test]
fn verify_password_truncated_hash_returns_error() {
    let result = verify_password("some-password", "$argon2");
    assert!(result.is_err());
}

#[test]
fn verify_password_multiple_correct_roundtrips() {
    let passwords = [
        "password1234",
        "another-pass-word",
        "c0mpl3x!p@ssw0rd",
        "very-long-password-with-many-characters",
    ];
    for pw in &passwords {
        let hash = hash_password(pw).unwrap();
        assert!(
            verify_password(pw, &hash).unwrap(),
            "Failed for: {}",
            pw
        );
    }
}

// ── PasswordProtectionState tests ─────────────────────────────────────────────

#[test]
fn protection_state_equality_uninitialized() {
    assert_eq!(
        PasswordProtectionState::Uninitialized,
        PasswordProtectionState::Uninitialized
    );
}

#[test]
fn protection_state_equality_sealed() {
    assert_eq!(PasswordProtectionState::Sealed, PasswordProtectionState::Sealed);
}

#[test]
fn protection_state_equality_tampered() {
    assert_eq!(PasswordProtectionState::Tampered, PasswordProtectionState::Tampered);
}

#[test]
fn protection_state_equality_legacy_unsealed() {
    assert_eq!(
        PasswordProtectionState::LegacyUnsealed,
        PasswordProtectionState::LegacyUnsealed
    );
}

#[test]
fn protection_state_inequality() {
    assert_ne!(
        PasswordProtectionState::Sealed,
        PasswordProtectionState::Tampered
    );
}

#[test]
fn protection_state_copy() {
    let s = PasswordProtectionState::Sealed;
    let s2 = s;
    assert_eq!(s, s2);
}

#[test]
fn protection_state_debug_uninitialized() {
    let s = format!("{:?}", PasswordProtectionState::Uninitialized);
    assert!(s.contains("Uninitialized"));
}

#[test]
fn protection_state_debug_sealed() {
    let s = format!("{:?}", PasswordProtectionState::Sealed);
    assert!(s.contains("Sealed"));
}

#[test]
fn protection_state_debug_tampered() {
    let s = format!("{:?}", PasswordProtectionState::Tampered);
    assert!(s.contains("Tampered"));
}

#[test]
fn protection_state_debug_legacy() {
    let s = format!("{:?}", PasswordProtectionState::LegacyUnsealed);
    assert!(s.contains("Legacy"));
}

// ── describe_protection_state tests ───────────────────────────────────────────

#[test]
fn describe_uninitialized() {
    assert_eq!(
        describe_protection_state(PasswordProtectionState::Uninitialized),
        "uninitialized"
    );
}

#[test]
fn describe_legacy_unsealed() {
    assert_eq!(
        describe_protection_state(PasswordProtectionState::LegacyUnsealed),
        "legacy-unsealed"
    );
}

#[test]
fn describe_sealed() {
    assert_eq!(
        describe_protection_state(PasswordProtectionState::Sealed),
        "device-sealed"
    );
}

#[test]
fn describe_tampered() {
    assert_eq!(
        describe_protection_state(PasswordProtectionState::Tampered),
        "tampered"
    );
}

#[test]
fn describe_returns_str_not_empty() {
    for state in [
        PasswordProtectionState::Uninitialized,
        PasswordProtectionState::LegacyUnsealed,
        PasswordProtectionState::Sealed,
        PasswordProtectionState::Tampered,
    ] {
        let desc = describe_protection_state(state);
        assert!(!desc.is_empty(), "Description empty for {:?}", state);
    }
}

#[test]
fn describe_all_states_unique() {
    let descs: Vec<&str> = [
        PasswordProtectionState::Uninitialized,
        PasswordProtectionState::LegacyUnsealed,
        PasswordProtectionState::Sealed,
        PasswordProtectionState::Tampered,
    ]
    .iter()
    .map(|&s| describe_protection_state(s))
    .collect();
    let unique: std::collections::HashSet<_> = descs.iter().copied().collect();
    assert_eq!(unique.len(), 4, "All descriptions should be unique");
}

// ── PasswordSetupArtifacts tests ──────────────────────────────────────────────

#[test]
fn setup_artifacts_protection_state_field_access() {
    let artifacts = rove_engine::security::local_auth::PasswordSetupArtifacts {
        protection_state: PasswordProtectionState::LegacyUnsealed,
        recovery_code: "RVE-ABCD-1234-5678".to_string(),
    };
    assert_eq!(artifacts.protection_state, PasswordProtectionState::LegacyUnsealed);
}

#[test]
fn setup_artifacts_recovery_code_field_access() {
    let artifacts = rove_engine::security::local_auth::PasswordSetupArtifacts {
        protection_state: PasswordProtectionState::Sealed,
        recovery_code: "RVE-ABCD-1234-5678".to_string(),
    };
    assert_eq!(artifacts.recovery_code, "RVE-ABCD-1234-5678");
}

#[test]
fn setup_artifacts_clone() {
    let artifacts = rove_engine::security::local_auth::PasswordSetupArtifacts {
        protection_state: PasswordProtectionState::Sealed,
        recovery_code: "RVE-TEST-1234-5678".to_string(),
    };
    let cloned = artifacts.clone();
    assert_eq!(cloned.recovery_code, artifacts.recovery_code);
    assert_eq!(cloned.protection_state, artifacts.protection_state);
}
