use std::net::ToSocketAddrs;
use std::time::Duration;

use crate::config::Config;
use crate::secrets::SecretManager;

use super::{DaemonManager, ProviderAvailability};

impl DaemonManager {
    pub(super) async fn check_provider_availability(config: &Config) -> ProviderAvailability {
        let secret_manager = SecretManager::new("rove");

        ProviderAvailability {
            ollama: Self::check_ollama_availability(&config.llm.ollama.base_url),
            openai: secret_manager.has_secret("openai_api_key").await,
            anthropic: secret_manager.has_secret("anthropic_api_key").await,
            gemini: secret_manager.has_secret("gemini_api_key").await,
            nvidia_nim: secret_manager.has_secret("nvidia_nim_api_key").await,
        }
    }

    fn check_ollama_availability(base_url: &str) -> bool {
        let url = base_url
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let parts: Vec<&str> = url.split(':').collect();
        if parts.len() != 2 {
            return false;
        }

        let host = parts[0];
        let port: u16 = match parts[1].parse() {
            Ok(port) => port,
            Err(_) => return false,
        };

        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from((
                match host.parse::<std::net::IpAddr>() {
                    Ok(ip) => ip,
                    Err(_) => match (host, port).to_socket_addrs() {
                        Ok(mut addrs) => match addrs.next() {
                            Some(addr) => addr.ip(),
                            None => return false,
                        },
                        Err(_) => return false,
                    },
                },
                port,
            )),
            Duration::from_secs(2),
        )
        .is_ok()
    }
}
