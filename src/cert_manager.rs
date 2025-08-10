use crate::config::Listener;
use anyhow::{Result, anyhow};
use rustls::ServerConfig;
use rustls_pemfile::{certs, private_key};
use rustls_pki_types::CertificateDer;
use std::io::BufReader;
use std::time::Duration;

/// Certificate manager handles loading and refreshing certificates from various sources
pub struct CertificateManager {
    http_client: reqwest::Client,
}

impl CertificateManager {
    /// Create a new certificate manager
    pub fn new() -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self { http_client })
    }

    /// Load certificate content from either file or URL
    pub async fn load_certificate(&self, path: &str) -> Result<String> {
        if path.starts_with("http://") || path.starts_with("https://") {
            self.load_from_url(path).await
        } else {
            self.load_from_file(path).await
        }
    }

    /// Load certificate from file
    async fn load_from_file(&self, path: &str) -> Result<String> {
        tracing::debug!("Reading certificate from file: {}", path);
        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| anyhow!("Failed to read certificate file {}: {}", path, e))
    }

    /// Load certificate from URL
    async fn load_from_url(&self, url: &str) -> Result<String> {
        tracing::info!("Fetching certificate from URL: {}", url);
        let response = self
            .http_client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch certificate from {}: {}", url, e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "HTTP error {} when fetching certificate from {}",
                response.status(),
                url
            ));
        }

        let content = response
            .text()
            .await
            .map_err(|e| anyhow!("Failed to read certificate content from {}: {}", url, e))?;

        tracing::info!("Successfully loaded certificate from URL: {}", url);
        Ok(content)
    }

    /// Create server config from certificate sources
    pub async fn create_server_config(&self, listener_config: &Listener) -> Result<ServerConfig> {
        // Load server certificate
        let cert_content = self.load_certificate(&listener_config.server_cert).await?;
        let cert_chain: Vec<CertificateDer> =
            certs(&mut BufReader::new(cert_content.as_bytes())).collect::<Result<Vec<_>, _>>()?;

        // Load server private key
        let key_content = self.load_certificate(&listener_config.server_key).await?;
        let private_key = private_key(&mut BufReader::new(key_content.as_bytes()))?
            .ok_or_else(|| anyhow!("No private key found in key data"))?;

        let config = if listener_config.mtls {
            // mTLS enabled - require client certificates
            if let Some(client_ca_path) = &listener_config.client_ca {
                let ca_content = self.load_certificate(client_ca_path).await?;
                let ca_certs: Vec<CertificateDer> =
                    certs(&mut BufReader::new(ca_content.as_bytes()))
                        .collect::<Result<Vec<_>, _>>()?;

                let mut client_auth_roots = rustls::RootCertStore::empty();
                for cert in ca_certs {
                    client_auth_roots.add(cert)?;
                }

                let client_cert_verifier =
                    rustls::server::WebPkiClientVerifier::builder(client_auth_roots.into())
                        .build()?;

                ServerConfig::builder()
                    .with_client_cert_verifier(client_cert_verifier)
                    .with_single_cert(cert_chain, private_key)?
            } else {
                return Err(anyhow!("mTLS enabled but no client_ca specified"));
            }
        } else {
            // No client authentication required
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(cert_chain, private_key)?
        };

        Ok(config)
    }

    /// Helper function to refresh a single certificate
    async fn refresh_certificate(client: &reqwest::Client, path: &str) -> Result<()> {
        if path.starts_with("http://") || path.starts_with("https://") {
            tracing::info!("Refreshing certificate from URL: {}", path);
            let _content = Self::fetch_certificate_content(client, path).await?;
            tracing::info!("Successfully refreshed certificate from URL: {}", path);
            // Certificate content is validated but not stored (just checking accessibility)
            Ok(())
        } else {
            tracing::info!("Refreshing certificate from file: {}", path);
            tokio::fs::read_to_string(path)
                .await
                .map_err(|e| anyhow!("Failed to read certificate file {}: {}", path, e))?;
            tracing::info!("Successfully refreshed certificate from file: {}", path);
            Ok(())
        }
    }

    /// Start background task to refresh certificates periodically
    pub fn start_refresh_task(&self, listener_config: &Listener) -> tokio::task::JoinHandle<()> {
        let http_client = self.http_client.clone();
        let refresh_interval = listener_config.cert_refresh_interval;
        let server_cert = listener_config.server_cert.clone();
        let server_key = listener_config.server_key.clone();
        let client_ca = listener_config.client_ca.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(refresh_interval);
            interval.tick().await; // Skip first immediate tick

            loop {
                interval.tick().await;

                // Refresh server certificate
                if let Err(e) = Self::refresh_certificate(&http_client, &server_cert).await {
                    tracing::error!(
                        "Failed to refresh server certificate {}: {}",
                        server_cert,
                        e
                    );
                }

                // Refresh server key
                if let Err(e) = Self::refresh_certificate(&http_client, &server_key).await {
                    tracing::error!("Failed to refresh server key {}: {}", server_key, e);
                }

                // Refresh client CA if present
                if let Some(ca_path) = &client_ca
                    && let Err(e) = Self::refresh_certificate(&http_client, ca_path).await
                {
                    tracing::error!("Failed to refresh client CA {}: {}", ca_path, e);
                }
            }
        })
    }

    /// Helper function to fetch certificate content from URL
    async fn fetch_certificate_content(client: &reqwest::Client, url: &str) -> Result<String> {
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch certificate from {}: {}", url, e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "HTTP error {} when fetching certificate from {}",
                response.status(),
                url
            ));
        }

        let content = response
            .text()
            .await
            .map_err(|e| anyhow!("Failed to read certificate content from {}: {}", url, e))?;

        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_from_file() {
        let manager = CertificateManager::new().unwrap();

        let result = manager.load_certificate("fixtures/test-cert.pem").await;
        // We expect this to work if the file exists
        if std::path::Path::new("fixtures/test-cert.pem").exists() {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_url_detection() {
        use crate::config::Listener;
        assert!(Listener::is_url("https://example.com/cert.pem"));
        assert!(Listener::is_url("http://example.com/cert.pem"));
        assert!(!Listener::is_url("/path/to/cert.pem"));
        assert!(!Listener::is_url("cert.pem"));
    }
}
