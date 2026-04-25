use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalTlsStatus {
    pub enabled: bool,
    pub cert_path: String,
    pub key_path: String,
}

pub fn localhost_tls_status() -> LocalTlsStatus {
    let (cert_path, key_path) = localhost_cert_paths();
    LocalTlsStatus {
        enabled: cert_path.exists() && key_path.exists(),
        cert_path: cert_path.display().to_string(),
        key_path: key_path.display().to_string(),
    }
}

pub fn localhost_cert_paths() -> (PathBuf, PathBuf) {
    let cert_dir = crate::config::paths::rove_home().join("certs");
    (
        cert_dir.join("localhost.pem"),
        cert_dir.join("localhost-key.pem"),
    )
}
