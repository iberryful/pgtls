use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(rename = "proxy", default)]
    pub proxies: Vec<Proxy>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Proxy {
    pub listener: Listener,
    pub backend: Backend,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Listener {
    pub bind_address: String,
    pub server_cert: String,
    pub server_key: String,
    #[serde(default)]
    pub mtls: bool,
    pub client_ca: Option<String>,
    #[serde(default = "default_refresh_interval", with = "parse_duration")]
    pub cert_refresh_interval: std::time::Duration,
}

fn default_refresh_interval() -> std::time::Duration {
    std::time::Duration::from_secs(24 * 3600) // 24 hours
}

mod parse_duration {
    use serde::{self, Deserialize, Deserializer};
    use std::time::Duration;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_duration_string(&s).map_err(serde::de::Error::custom)
    }

    fn parse_duration_string(s: &str) -> Result<Duration, String> {
        let s = s.trim();

        if let Some(hours_str) = s.strip_suffix('h') {
            let hours: u64 = hours_str
                .parse()
                .map_err(|_| format!("Invalid hours: {hours_str}"))?;
            Ok(Duration::from_secs(hours * 3600))
        } else if let Some(minutes_str) = s.strip_suffix("min") {
            let minutes: u64 = minutes_str
                .parse()
                .map_err(|_| format!("Invalid minutes: {minutes_str}"))?;
            Ok(Duration::from_secs(minutes * 60))
        } else if let Some(seconds_str) = s.strip_suffix('s') {
            let seconds: u64 = seconds_str
                .parse()
                .map_err(|_| format!("Invalid seconds: {seconds_str}"))?;
            Ok(Duration::from_secs(seconds))
        } else {
            // Try parsing as raw seconds
            let seconds: u64 = s
                .parse()
                .map_err(|_| format!("Invalid duration format: {s}"))?;
            Ok(Duration::from_secs(seconds))
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Backend {
    pub address: String,
}

impl Listener {
    pub fn is_url(path: &str) -> bool {
        path.starts_with("http://") || path.starts_with("https://")
    }

    #[allow(dead_code)]
    pub fn server_cert_is_url(&self) -> bool {
        Self::is_url(&self.server_cert)
    }

    #[allow(dead_code)]
    pub fn server_key_is_url(&self) -> bool {
        Self::is_url(&self.server_key)
    }

    #[allow(dead_code)]
    pub fn client_ca_is_url(&self) -> bool {
        self.client_ca.as_ref().is_some_and(|ca| Self::is_url(ca))
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        // Read the configuration file
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read configuration file: {path}"))?;

        // Parse TOML
        let config: Config =
            toml::from_str(&content).with_context(|| "Failed to parse TOML configuration")?;

        // Validate configuration
        config.validate()?;

        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.proxies.is_empty() {
            return Err(anyhow!("At least one proxy configuration is required"));
        }

        for (i, proxy) in self.proxies.iter().enumerate() {
            proxy.validate_listener(i)?;
            proxy.validate_backend(i)?;
        }

        Ok(())
    }
}

impl Proxy {
    fn validate_listener(&self, index: usize) -> Result<()> {
        let prefix = format!("proxy[{index}].listener");

        // Validate server certificate and key sources
        self.validate_cert_source(&self.listener.server_cert, &format!("{prefix}.server_cert"))?;
        self.validate_cert_source(&self.listener.server_key, &format!("{prefix}.server_key"))?;

        // If mTLS is enabled, client_ca must be present and valid
        if self.listener.mtls {
            match &self.listener.client_ca {
                Some(client_ca) => {
                    self.validate_cert_source(client_ca, &format!("{prefix}.client_ca"))?;
                }
                None => {
                    return Err(anyhow!(
                        "{}.client_ca is required when mtls is true",
                        prefix
                    ));
                }
            }
        }

        Ok(())
    }

    fn validate_cert_source(&self, cert_source: &str, field_name: &str) -> Result<()> {
        if Listener::is_url(cert_source) {
            // Validate URL format
            if !cert_source.starts_with("https://") && !cert_source.starts_with("http://") {
                return Err(anyhow!(
                    "Invalid URL format for {}: {}",
                    field_name,
                    cert_source
                ));
            }
        } else {
            // File path - check if file exists
            self.check_file_exists(cert_source, field_name)?;
        }

        Ok(())
    }

    fn validate_backend(&self, index: usize) -> Result<()> {
        let _prefix = format!("proxy[{index}].backend");
        // No validation needed for plaintext-only backends
        Ok(())
    }

    fn check_file_exists(&self, path: &str, field_name: &str) -> Result<()> {
        if !Path::new(path).exists() {
            return Err(anyhow!("File not found for {}: {}", field_name, path));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    fn create_temp_file(content: &str) -> NamedTempFile {
        let temp_file = NamedTempFile::new().expect("Failed to create temporary file");
        fs::write(temp_file.path(), content).expect("Failed to write to temporary file");
        temp_file
    }

    fn create_dummy_cert_files() -> (NamedTempFile, NamedTempFile, NamedTempFile, NamedTempFile) {
        let server_cert = create_temp_file("dummy server cert");
        let server_key = create_temp_file("dummy server key");
        let client_ca = create_temp_file("dummy client ca");
        let backend_ca = create_temp_file("dummy backend ca");
        (server_cert, server_key, client_ca, backend_ca)
    }

    #[test]
    fn test_load_full_config() {
        let (server_cert, server_key, client_ca, _backend_ca) = create_dummy_cert_files();

        let config_content = format!(
            r#"
log_level = "debug"

[[proxy]]
  [proxy.listener]
  bind_address = "0.0.0.0:6432"
  server_cert = "{}"
  server_key = "{}"
  mtls = true
  client_ca = "{}"

  [proxy.backend]
  address = "db.example.com:5432"

[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6433"
  server_cert = "{}"
  server_key = "{}"
  mtls = false

  [proxy.backend]
  address = "10.0.1.50:5432"
"#,
            server_cert.path().display(),
            server_key.path().display(),
            client_ca.path().display(),
            server_cert.path().display(),
            server_key.path().display(),
        );

        let config_file = create_temp_file(&config_content);
        let config = Config::load(config_file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.log_level, "debug");
        assert_eq!(config.proxies.len(), 2);

        // First proxy
        let proxy1 = &config.proxies[0];
        assert_eq!(proxy1.listener.bind_address, "0.0.0.0:6432");
        assert!(proxy1.listener.mtls);
        assert!(proxy1.listener.client_ca.is_some());
        assert_eq!(proxy1.backend.address, "db.example.com:5432");

        // Second proxy
        let proxy2 = &config.proxies[1];
        assert_eq!(proxy2.listener.bind_address, "127.0.0.1:6433");
        assert!(!proxy2.listener.mtls);
        assert!(proxy2.listener.client_ca.is_none());
        assert_eq!(proxy2.backend.address, "10.0.1.50:5432");
    }

    #[test]
    fn test_load_minimal_config() {
        let (server_cert, server_key, _, _backend_ca) = create_dummy_cert_files();

        let config_content = format!(
            r#"
[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6432"
  server_cert = "{}"
  server_key = "{}"

  [proxy.backend]
  address = "localhost:5432"
"#,
            server_cert.path().display(),
            server_key.path().display(),
        );

        let config_file = create_temp_file(&config_content);
        let config = Config::load(config_file.path().to_str().unwrap()).unwrap();

        // Check defaults
        assert_eq!(config.log_level, "info"); // default
        assert_eq!(config.proxies.len(), 1);

        let proxy = &config.proxies[0];
        assert!(!proxy.listener.mtls); // default false
    }

    #[test]
    fn test_validation_mtls_without_client_ca() {
        let (server_cert, server_key, _, _) = create_dummy_cert_files();

        let config_content = format!(
            r#"
[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6432"
  server_cert = "{}"
  server_key = "{}"
  mtls = true

  [proxy.backend]
  address = "localhost:5432"
"#,
            server_cert.path().display(),
            server_key.path().display(),
        );

        let config_file = create_temp_file(&config_content);
        let result = Config::load(config_file.path().to_str().unwrap());

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("client_ca is required when mtls is true")
        );
    }

    #[test]
    fn test_file_not_found() {
        let result = Config::load("/non/existent/file.toml");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to read configuration file")
        );
    }

    #[test]
    fn test_missing_cert_file() {
        let config_content = r#"
[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6432"
  server_cert = "/non/existent/cert.pem"
  server_key = "/non/existent/key.pem"

  [proxy.backend]
  address = "localhost:5432"
"#;

        let config_file = create_temp_file(config_content);
        let result = Config::load(config_file.path().to_str().unwrap());

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("File not found"));
    }

    #[test]
    fn test_empty_proxies() {
        let config_content = r#"
log_level = "info"
"#;

        let config_file = create_temp_file(config_content);
        let result = Config::load(config_file.path().to_str().unwrap());

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("At least one proxy configuration is required")
        );
    }

    #[test]
    fn test_cert_refresh_interval_parsing() {
        let (server_cert, server_key, _, _) = create_dummy_cert_files();

        let config_content = format!(
            r#"
[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6432"
  cert_refresh_interval = "12h"
  server_cert = "{}"
  server_key = "{}"

  [proxy.backend]
  address = "localhost:5432"
"#,
            server_cert.path().display(),
            server_key.path().display(),
        );

        let config_file = create_temp_file(&config_content);
        let config = Config::load(config_file.path().to_str().unwrap()).unwrap();

        let proxy = &config.proxies[0];
        assert_eq!(proxy.listener.cert_refresh_interval.as_secs(), 12 * 3600);
    }

    #[test]
    fn test_url_certificate_with_refresh() {
        // Test URL configuration format - this will fail validation but should parse
        let config_content = r#"
[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6432"
  cert_refresh_interval = "6h"
  server_cert = "https://example.com/server.pem"
  server_key = "https://example.com/server.key"

  [proxy.backend]
  address = "localhost:5432"
"#;

        let config_file = create_temp_file(config_content);
        let result =
            toml::from_str::<Config>(&std::fs::read_to_string(config_file.path()).unwrap());

        // Should parse successfully (validation will fail since URLs don't exist, but structure is correct)
        assert!(result.is_ok());
        let config = result.unwrap();

        let proxy = &config.proxies[0];
        assert_eq!(proxy.listener.cert_refresh_interval.as_secs(), 6 * 3600);
        assert!(proxy.listener.server_cert_is_url());
        assert!(proxy.listener.server_key_is_url());
        assert!(!proxy.listener.client_ca_is_url());
    }
}
