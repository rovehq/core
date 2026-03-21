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
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let cert_dir = home.join(".rove").join("certs");
    (
        cert_dir.join("localhost.pem"),
        cert_dir.join("localhost-key.pem"),
    )
}
