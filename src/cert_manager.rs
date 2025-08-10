use crate::config::Listener;
use anyhow::{Result, anyhow};
use rustls::ServerConfig;
use rustls_pemfile::{certs, private_key};
use rustls_pki_types::CertificateDer;
use std::io::BufReader;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Certificate data loaded from either file or URL
#[derive(Debug, Clone)]
pub struct CertificateData {
    pub content: String,
    pub loaded_at: Instant,
    pub refresh_interval: Duration,
}

/// Certificate manager handles loading and refreshing certificates from various sources
pub struct CertificateManager {
    http_client: reqwest::Client,
    cert_cache: Arc<RwLock<std::collections::HashMap<String, CertificateData>>>,
}

impl CertificateManager {
    /// Create a new certificate manager
    pub fn new() -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            http_client,
            cert_cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
        })
    }

    /// Load certificate content from either file or URL
    pub async fn load_certificate(&self, path: &str, refresh_interval: Duration) -> Result<String> {
        if path.starts_with("http://") || path.starts_with("https://") {
            self.load_from_url(path, refresh_interval).await
        } else {
            self.load_from_file_cached(path, refresh_interval).await
        }
    }

    /// Load certificate from file with caching for refresh intervals
    async fn load_from_file_cached(
        &self,
        path: &str,
        refresh_interval: Duration,
    ) -> Result<String> {
        // Check cache first
        {
            let cache = self.cert_cache.read().await;
            if let Some(cached_data) = cache.get(path) {
                if cached_data.loaded_at.elapsed() < cached_data.refresh_interval {
                    tracing::debug!("Using cached certificate for file: {}", path);
                    return Ok(cached_data.content.clone());
                }
            }
        }

        // Read file content
        tracing::debug!("Reading certificate from file: {}", path);
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| anyhow!("Failed to read certificate file {}: {}", path, e))?;

        // Cache the certificate
        {
            let mut cache = self.cert_cache.write().await;
            cache.insert(
                path.to_string(),
                CertificateData {
                    content: content.clone(),
                    loaded_at: Instant::now(),
                    refresh_interval,
                },
            );
        }

        Ok(content)
    }

    /// Load certificate from URL with caching
    async fn load_from_url(&self, url: &str, refresh_interval: Duration) -> Result<String> {
        // Check cache first
        {
            let cache = self.cert_cache.read().await;
            if let Some(cached_data) = cache.get(url) {
                if cached_data.loaded_at.elapsed() < cached_data.refresh_interval {
                    tracing::debug!("Using cached certificate for URL: {}", url);
                    return Ok(cached_data.content.clone());
                }
            }
        }

        // Fetch from URL
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

        // Validate that content looks like a certificate
        if !content.contains("-----BEGIN CERTIFICATE-----")
            && !content.contains("-----BEGIN RSA PRIVATE KEY-----")
            && !content.contains("-----BEGIN PRIVATE KEY-----")
        {
            return Err(anyhow!("Invalid certificate format from URL: {}", url));
        }

        // Cache the certificate
        {
            let mut cache = self.cert_cache.write().await;
            cache.insert(
                url.to_string(),
                CertificateData {
                    content: content.clone(),
                    loaded_at: Instant::now(),
                    refresh_interval,
                },
            );
        }

        tracing::info!("Successfully loaded certificate from URL: {}", url);
        Ok(content)
    }

    /// Create server config from certificate sources
    pub async fn create_server_config(&self, listener_config: &Listener) -> Result<ServerConfig> {
        // Load server certificate
        let cert_content = self
            .load_certificate(
                &listener_config.server_cert,
                listener_config.cert_refresh_interval,
            )
            .await?;
        let cert_chain: Vec<CertificateDer> =
            certs(&mut BufReader::new(cert_content.as_bytes())).collect::<Result<Vec<_>, _>>()?;

        // Load server private key
        let key_content = self
            .load_certificate(
                &listener_config.server_key,
                listener_config.cert_refresh_interval,
            )
            .await?;
        let private_key = private_key(&mut BufReader::new(key_content.as_bytes()))?
            .ok_or_else(|| anyhow!("No private key found in key data"))?;

        let config = if listener_config.mtls {
            // mTLS enabled - require client certificates
            if let Some(client_ca_path) = &listener_config.client_ca {
                let ca_content = self
                    .load_certificate(client_ca_path, listener_config.cert_refresh_interval)
                    .await?;
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

    /// Start background task to refresh certificates
    pub fn start_refresh_task(&self) -> tokio::task::JoinHandle<()> {
        let cache = self.cert_cache.clone();
        let http_client = self.http_client.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3600)); // Check every hour

            loop {
                interval.tick().await;

                let sources_to_refresh = {
                    let cache_read = cache.read().await;
                    cache_read
                        .iter()
                        .filter_map(|(source, cached_data)| {
                            if cached_data.loaded_at.elapsed() >= cached_data.refresh_interval {
                                Some((source.clone(), cached_data.refresh_interval))
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                };

                for (source, refresh_interval) in sources_to_refresh {
                    if source.starts_with("http://") || source.starts_with("https://") {
                        tracing::info!("Refreshing expired certificate from URL: {}", source);

                        match Self::fetch_certificate_content(&http_client, &source).await {
                            Ok(content) => {
                                let mut cache_write = cache.write().await;
                                cache_write.insert(
                                    source.clone(),
                                    CertificateData {
                                        content,
                                        loaded_at: Instant::now(),
                                        refresh_interval,
                                    },
                                );
                                tracing::info!(
                                    "Successfully refreshed certificate from URL: {}",
                                    source
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to refresh certificate from URL {}: {}",
                                    source,
                                    e
                                );
                                // Keep the old certificate data for fallback
                            }
                        }
                    } else {
                        // File-based certificate with refresh interval
                        tracing::info!("Refreshing expired certificate from file: {}", source);

                        match tokio::fs::read_to_string(&source).await {
                            Ok(content) => {
                                let mut cache_write = cache.write().await;
                                cache_write.insert(
                                    source.clone(),
                                    CertificateData {
                                        content,
                                        loaded_at: Instant::now(),
                                        refresh_interval,
                                    },
                                );
                                tracing::info!(
                                    "Successfully refreshed certificate from file: {}",
                                    source
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to refresh certificate from file {}: {}",
                                    source,
                                    e
                                );
                                // Keep the old certificate data for fallback
                            }
                        }
                    }
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

        // Validate that content looks like a certificate
        if !content.contains("-----BEGIN CERTIFICATE-----")
            && !content.contains("-----BEGIN RSA PRIVATE KEY-----")
            && !content.contains("-----BEGIN PRIVATE KEY-----")
        {
            return Err(anyhow!("Invalid certificate format from URL: {}", url));
        }

        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_from_file() {
        let manager = CertificateManager::new().unwrap();

        let result = manager
            .load_certificate("fixtures/test-cert.pem", Duration::from_secs(3600))
            .await;
        // We expect this to work if the file exists
        if std::path::Path::new("fixtures/test-cert.pem").exists() {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_url_detection() {
        assert!(Listener::is_url("https://example.com/cert.pem"));
        assert!(Listener::is_url("http://example.com/cert.pem"));
        assert!(!Listener::is_url("/path/to/cert.pem"));
        assert!(!Listener::is_url("cert.pem"));
    }
}
