//! Build script for embedding team public keys at compile time.
//!
//! Two production keys are embedded separately:
//!
//! 1. **Official** — signs engine releases, core-tools, official/reviewed
//!    plugins & drivers, brains, `revoked.json`, and the community-manifest
//!    wrapper. Lives offline / HSM, rotated rarely.
//!    - env:  `ROVE_TEAM_OFFICIAL_PUBLIC_KEY` (hex or base64)
//!    - file: `manifest/team_official_public_key.bin` (raw 32 bytes)
//!    - file: `manifest/team_official_public_key.hex`
//!
//! 2. **Community** — signs community-tier plugin manifests on PR merge.
//!    Lives in CI secret, rotated independently.
//!    - env:  `ROVE_TEAM_COMMUNITY_PUBLIC_KEY` (hex or base64)
//!    - file: `manifest/team_community_public_key.bin`
//!    - file: `manifest/team_community_public_key.hex`
//!
//! Dev fallback: `manifest/dev_public_key.bin` is embedded for non-production
//! builds (see `security/crypto/mod.rs`).
//!
//! If either key is missing, a placeholder zero key is generated. Placeholder
//! signatures are rejected at runtime under `--features production`.

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

    // 3. Stamp the release channel baked into this build.
    let channel = if env::var_os("CARGO_FEATURE_CHANNEL_DEV").is_some() {
        "dev"
    } else {
        "stable"
    };
    println!("cargo:rustc-env=ROVE_BUILD_CHANNEL={}", channel);
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_CHANNEL_DEV");

    let out_dir = env::var("OUT_DIR")?;

    // Official key — signs engine, core-tools, official/reviewed plugins &
    // drivers, brains, revoked.json, community-manifest wrapper.
    let official_key_bytes = load_public_key(
        "ROVE_TEAM_OFFICIAL_PUBLIC_KEY",
        &["manifest/team_official_public_key.bin"],
        &["manifest/team_official_public_key.hex"],
    );
    fs::write(
        PathBuf::from(&out_dir).join("team_official_public_key.bin"),
        &official_key_bytes,
    )?;

    // Community key — signs community-tier plugin manifests on PR merge.
    // Lives in CI secret, rotated independently.
    let community_key_bytes = load_public_key(
        "ROVE_TEAM_COMMUNITY_PUBLIC_KEY",
        &["manifest/team_community_public_key.bin"],
        &["manifest/team_community_public_key.hex"],
    );
    fs::write(
        PathBuf::from(&out_dir).join("team_community_public_key.bin"),
        &community_key_bytes,
    )?;

    let dev_key_bytes = load_dev_public_key();
    fs::write(
        PathBuf::from(&out_dir).join("dev_public_key.bin"),
        &dev_key_bytes,
    )?;

    println!("cargo:rerun-if-changed=manifest/team_official_public_key.bin");
    println!("cargo:rerun-if-changed=manifest/team_official_public_key.hex");
    println!("cargo:rerun-if-changed=manifest/team_community_public_key.bin");
    println!("cargo:rerun-if-changed=manifest/team_community_public_key.hex");
    println!("cargo:rerun-if-changed=manifest/dev_public_key.bin");
    println!("cargo:rerun-if-env-changed=ROVE_TEAM_OFFICIAL_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=ROVE_TEAM_COMMUNITY_PUBLIC_KEY");

    if is_placeholder_key(&official_key_bytes) {
        println!("cargo:warning=Using placeholder OFFICIAL public key for development");
        println!("cargo:warning=Production builds MUST provide ROVE_TEAM_OFFICIAL_PUBLIC_KEY or manifest/team_official_public_key.bin");
    }
    if is_placeholder_key(&community_key_bytes) {
        println!("cargo:warning=Using placeholder COMMUNITY public key for development");
        println!("cargo:warning=Production builds MUST provide ROVE_TEAM_COMMUNITY_PUBLIC_KEY or manifest/team_community_public_key.bin");
    }

    Ok(())
}

/// Generic key loader used by both Official and Community keys.
///
/// Resolution order:
/// 1. Env var (hex or base64 DER)
/// 2. Binary file (`manifest/<name>.bin`, raw 32 bytes)
/// 3. Hex file (`manifest/<name>.hex`)
/// 4. Placeholder zero key — rejected by production builds at runtime.
fn load_public_key(env_var: &str, bin_paths: &[&str], hex_paths: &[&str]) -> Vec<u8> {
    if let Ok(key_str) = env::var(env_var) {
        if let Some(bytes) = parse_env_key(&key_str, env_var) {
            return bytes;
        }
    }

    let workspace_root = get_workspace_root();
    for rel in bin_paths {
        let path = workspace_root.join(rel);
        if path.exists() {
            if let Ok(bytes) = fs::read(&path) {
                if bytes.len() == 32 {
                    println!("cargo:warning=Loaded key from {}", rel);
                    return bytes;
                }
            }
            println!("cargo:warning=Invalid {} (must be 32 bytes)", rel);
        }
    }
    for rel in hex_paths {
        let path = workspace_root.join(rel);
        if path.exists() {
            if let Ok(hex_str) = fs::read_to_string(&path) {
                if let Ok(bytes) = hex::decode(hex_str.trim()) {
                    if bytes.len() == 32 {
                        println!("cargo:warning=Loaded key from {}", rel);
                        return bytes;
                    }
                }
            }
            println!("cargo:warning=Invalid {} (must be 32 bytes hex)", rel);
        }
    }

    println!(
        "cargo:warning=No key for {}, generating placeholder",
        env_var
    );
    generate_placeholder_key()
}

fn parse_env_key(key_str: &str, env_var: &str) -> Option<Vec<u8>> {
    let key_str = key_str.trim();
    if let Ok(bytes) = hex::decode(key_str) {
        if bytes.len() == 32 {
            println!("cargo:warning=Loaded key from {} (hex)", env_var);
            return Some(bytes);
        }
    }
    use base64::Engine;
    if let Ok(der_bytes) = base64::engine::general_purpose::STANDARD.decode(key_str) {
        if der_bytes.len() == 44 {
            let ed25519_der_prefix: [u8; 12] = [
                0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
            ];
            if der_bytes[..12] == ed25519_der_prefix {
                println!("cargo:warning=Loaded key from {} (base64 DER)", env_var);
                return Some(der_bytes[12..].to_vec());
            }
        }
        if der_bytes.len() == 32 {
            println!("cargo:warning=Loaded key from {} (base64 raw)", env_var);
            return Some(der_bytes);
        }
    }
    println!(
        "cargo:warning=Invalid {} (must be 32 bytes hex or base64-encoded Ed25519 public key)",
        env_var
    );
    None
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
