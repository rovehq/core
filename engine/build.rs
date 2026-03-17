//! Build script for embedding the team public key at compile time
//!
//! This script reads the team public key from the manifest directory and
//! embeds it into the binary. This ensures the key cannot be modified
//! without recompiling the engine.
//!
//! # Key Location
//!
//! The script looks for the public key in the following locations (in order):
//! 1. Environment variable `ROVE_TEAM_PUBLIC_KEY` (hex-encoded)
//! 2. File `manifest/team_public_key.bin` (raw bytes)
//! 3. File `manifest/team_public_key.hex` (hex-encoded)
//! 4. File `manifest/dev_public_key.bin` (raw bytes, dev fallback)
//!
//! If no key is found, a placeholder key is generated for development builds.
//! **Production builds MUST provide a real key.**

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Get the current Git commit hash
    let commit_hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_COMMIT_HASH={}", commit_hash);

    // 2. Get the current Build Timestamp (ISO 8601)
    let build_time = chrono::Utc::now().to_rfc3339();
    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", build_time);

    let out_dir = env::var("OUT_DIR")?;

    // Load the team public key and write it to OUT_DIR
    // crypto/mod.rs will include_bytes! from OUT_DIR
    let public_key_bytes = load_team_public_key();
    let dest_path = PathBuf::from(&out_dir).join("team_public_key.bin");
    fs::write(&dest_path, &public_key_bytes)?;

    // Also write a dev key if we have one (for non-production builds)
    let dev_key_bytes = load_dev_public_key();
    let dev_dest_path = PathBuf::from(&out_dir).join("dev_public_key.bin");
    fs::write(&dev_dest_path, &dev_key_bytes)?;

    println!("cargo:rerun-if-changed=manifest/team_public_key.bin");
    println!("cargo:rerun-if-changed=manifest/team_public_key.hex");
    println!("cargo:rerun-if-changed=manifest/dev_public_key.bin");
    println!("cargo:rerun-if-env-changed=ROVE_TEAM_PUBLIC_KEY");

    // Warn if using placeholder key
    if is_placeholder_key(&public_key_bytes) {
        println!("cargo:warning=Using placeholder team public key for development");
        println!("cargo:warning=Production builds MUST provide a real key via:");
        println!("cargo:warning=  - ROVE_TEAM_PUBLIC_KEY environment variable");
        println!("cargo:warning=  - manifest/team_public_key.bin file");
        println!("cargo:warning=  - manifest/team_public_key.hex file");
    }

    Ok(())
}

/// Load the team public key from available sources
///
/// Priority order:
/// 1. ROVE_TEAM_PUBLIC_KEY environment variable (hex)
/// 2. manifest/team_public_key.bin (raw bytes)
/// 3. manifest/team_public_key.hex (hex string)
/// 4. Generate placeholder for development
fn load_team_public_key() -> Vec<u8> {
    // Try environment variable first (used by CI)
    if let Ok(key_str) = env::var("ROVE_TEAM_PUBLIC_KEY") {
        let key_str = key_str.trim();

        // Try hex decode first (64 hex chars = 32 bytes raw key)
        if let Ok(bytes) = hex::decode(key_str) {
            if bytes.len() == 32 {
                println!(
                    "cargo:warning=Loaded team public key from ROVE_TEAM_PUBLIC_KEY env var (hex)"
                );
                return bytes;
            }
        }

        // Try base64 decode (PEM content without headers, from generate_keys.py)
        // Ed25519 public keys in DER format are 44 bytes:
        //   12 bytes ASN.1 header + 32 bytes raw key
        use base64::Engine;
        if let Ok(der_bytes) = base64::engine::general_purpose::STANDARD.decode(key_str) {
            // Full DER-encoded Ed25519 public key (44 bytes)
            if der_bytes.len() == 44 {
                let ed25519_der_prefix: [u8; 12] = [
                    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
                ];
                if der_bytes[..12] == ed25519_der_prefix {
                    println!("cargo:warning=Loaded team public key from ROVE_TEAM_PUBLIC_KEY env var (base64 DER)");
                    return der_bytes[12..].to_vec();
                }
            }
            // Raw 32-byte key that was base64-encoded
            if der_bytes.len() == 32 {
                println!("cargo:warning=Loaded team public key from ROVE_TEAM_PUBLIC_KEY env var (base64 raw)");
                return der_bytes;
            }
        }

        println!("cargo:warning=Invalid ROVE_TEAM_PUBLIC_KEY (must be 32 bytes hex or base64-encoded Ed25519 public key)");
    }

    let workspace_root = get_workspace_root();

    // Try binary file
    let bin_path = workspace_root.join("manifest/team_public_key.bin");
    if bin_path.exists() {
        if let Ok(bytes) = fs::read(&bin_path) {
            if bytes.len() == 32 {
                println!("cargo:warning=Loaded team public key from manifest/team_public_key.bin");
                return bytes;
            }
        }
        println!("cargo:warning=Invalid manifest/team_public_key.bin (must be 32 bytes)");
    }

    // Try hex file
    let hex_path = workspace_root.join("manifest/team_public_key.hex");
    if hex_path.exists() {
        if let Ok(hex_str) = fs::read_to_string(&hex_path) {
            let hex_str = hex_str.trim();
            if let Ok(bytes) = hex::decode(hex_str) {
                if bytes.len() == 32 {
                    println!(
                        "cargo:warning=Loaded team public key from manifest/team_public_key.hex"
                    );
                    return bytes;
                }
            }
        }
        println!("cargo:warning=Invalid manifest/team_public_key.hex (must be 32 bytes hex)");
    }

    // Generate placeholder for development
    println!("cargo:warning=No team public key found, generating placeholder");
    generate_placeholder_key()
}

/// Load the dev public key from manifest/dev_public_key.bin
/// Falls back to placeholder if not found
fn load_dev_public_key() -> Vec<u8> {
    let workspace_root = get_workspace_root();
    let dev_path = workspace_root.join("manifest/dev_public_key.bin");
    if dev_path.exists() {
        if let Ok(bytes) = fs::read(&dev_path) {
            if bytes.len() == 32 {
                return bytes;
            }
        }
    }
    generate_placeholder_key()
}

/// Get workspace root directory
fn get_workspace_root() -> PathBuf {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf()
}

/// Generate a placeholder key for development builds
///
/// This key is deterministic so that development builds are reproducible.
/// It is clearly marked as a placeholder and should never be used in production.
fn generate_placeholder_key() -> Vec<u8> {
    vec![0u8; 32]
}

/// Check if a key is the placeholder key
fn is_placeholder_key(key: &[u8]) -> bool {
    key.len() == 32 && key.iter().all(|&b| b == 0)
}
