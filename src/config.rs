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
}

#[derive(Debug, Deserialize, Clone)]
pub struct Backend {
    pub address: String,
    #[serde(default = "default_tls_enabled")]
    pub tls_enabled: bool,
    pub root_ca: Option<String>,
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_tls_enabled() -> bool {
    false
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

        // Check if server certificate and key files exist
        self.check_file_exists(&self.listener.server_cert, &format!("{prefix}.server_cert"))?;
        self.check_file_exists(&self.listener.server_key, &format!("{prefix}.server_key"))?;

        // If mTLS is enabled, client_ca must be present and exist
        if self.listener.mtls {
            match &self.listener.client_ca {
                Some(client_ca) => {
                    self.check_file_exists(client_ca, &format!("{prefix}.client_ca"))?;
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

    fn validate_backend(&self, index: usize) -> Result<()> {
        let prefix = format!("proxy[{index}].backend");

        // If TLS is enabled, root_ca must be present and exist
        if self.backend.tls_enabled {
            match &self.backend.root_ca {
                Some(root_ca) => {
                    self.check_file_exists(root_ca, &format!("{prefix}.root_ca"))?;
                }
                None => {
                    return Err(anyhow!(
                        "{}.root_ca is required when tls_enabled is true",
                        prefix
                    ));
                }
            }
        }

        // If client_cert is provided, client_key must also be provided
        match (&self.backend.client_cert, &self.backend.client_key) {
            (Some(client_cert), Some(client_key)) => {
                self.check_file_exists(client_cert, &format!("{prefix}.client_cert"))?;
                self.check_file_exists(client_key, &format!("{prefix}.client_key"))?;
            }
            (Some(_), None) => {
                return Err(anyhow!(
                    "{}.client_key is required when client_cert is provided",
                    prefix
                ));
            }
            (None, Some(_)) => {
                return Err(anyhow!(
                    "{}.client_cert is required when client_key is provided",
                    prefix
                ));
            }
            (None, None) => {
                // Both are None, which is valid
            }
        }

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
        let (server_cert, server_key, client_ca, backend_ca) = create_dummy_cert_files();

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
  tls_enabled = true
  root_ca = "{}"

[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6433"
  server_cert = "{}"
  server_key = "{}"
  mtls = false

  [proxy.backend]
  address = "10.0.1.50:5432"
  tls_enabled = false
"#,
            server_cert.path().display(),
            server_key.path().display(),
            client_ca.path().display(),
            backend_ca.path().display(),
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
        assert!(proxy1.backend.tls_enabled);
        assert!(proxy1.backend.root_ca.is_some());

        // Second proxy
        let proxy2 = &config.proxies[1];
        assert_eq!(proxy2.listener.bind_address, "127.0.0.1:6433");
        assert!(!proxy2.listener.mtls);
        assert!(proxy2.listener.client_ca.is_none());
        assert_eq!(proxy2.backend.address, "10.0.1.50:5432");
        assert!(!proxy2.backend.tls_enabled);
        assert!(proxy2.backend.root_ca.is_none());
    }

    #[test]
    fn test_load_minimal_config() {
        let (server_cert, server_key, _, backend_ca) = create_dummy_cert_files();

        let config_content = format!(
            r#"
[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6432"
  server_cert = "{}"
  server_key = "{}"

  [proxy.backend]
  address = "localhost:5432"
  root_ca = "{}"
"#,
            server_cert.path().display(),
            server_key.path().display(),
            backend_ca.path().display(),
        );

        let config_file = create_temp_file(&config_content);
        let config = Config::load(config_file.path().to_str().unwrap()).unwrap();

        // Check defaults
        assert_eq!(config.log_level, "info"); // default
        assert_eq!(config.proxies.len(), 1);

        let proxy = &config.proxies[0];
        assert!(!proxy.listener.mtls); // default false
        assert!(!proxy.backend.tls_enabled); // default false
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
  tls_enabled = false
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
    fn test_validation_tls_without_root_ca() {
        let (server_cert, server_key, _, _) = create_dummy_cert_files();

        let config_content = format!(
            r#"
[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6432"
  server_cert = "{}"
  server_key = "{}"

  [proxy.backend]
  address = "localhost:5432"
  tls_enabled = true
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
                .contains("root_ca is required when tls_enabled is true")
        );
    }

    #[test]
    fn test_validation_client_cert_without_key() {
        let (server_cert, server_key, _, backend_ca) = create_dummy_cert_files();
        let client_cert = create_temp_file("dummy client cert");

        let config_content = format!(
            r#"
[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6432"
  server_cert = "{}"
  server_key = "{}"

  [proxy.backend]
  address = "localhost:5432"
  tls_enabled = true
  root_ca = "{}"
  client_cert = "{}"
"#,
            server_cert.path().display(),
            server_key.path().display(),
            backend_ca.path().display(),
            client_cert.path().display(),
        );

        let config_file = create_temp_file(&config_content);
        let result = Config::load(config_file.path().to_str().unwrap());

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("client_key is required when client_cert is provided")
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
  tls_enabled = false
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
}
