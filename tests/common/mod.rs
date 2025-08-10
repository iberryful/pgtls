use anyhow::Result;
use rcgen::{Certificate, CertificateParams, IsCa, KeyPair};
use rustls::ClientConfig;
use rustls_pemfile::{certs, private_key};
use rustls_pki_types::CertificateDer;
use std::fs::File;
use std::io::{BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use tempfile::TempDir;
use tokio::net::TcpStream;

/// Test certificate bundle containing generated cert, key, and CA
#[derive(Clone)]
pub struct TestCertBundle {
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_pem: String,
}

/// Generate a self-signed certificate for testing
pub fn generate_test_certificate(subject: &str) -> Result<TestCertBundle> {
    let mut params = CertificateParams::new(vec![subject.to_string()]);
    params.is_ca = IsCa::NoCa;

    let _key_pair = KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let cert = Certificate::from_params(params)?;

    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();

    Ok(TestCertBundle {
        cert_pem: cert_pem.clone(),
        key_pem,
        ca_pem: cert_pem, // For self-signed, cert is also the CA
    })
}

/// Generate a CA certificate and client certificate signed by that CA
pub fn generate_ca_and_client_certificate(
    ca_subject: &str,
    client_subject: &str,
) -> Result<(TestCertBundle, TestCertBundle)> {
    // Generate CA
    let mut ca_params = CertificateParams::new(vec![ca_subject.to_string()]);
    ca_params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);

    let _ca_key_pair = KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let ca_cert = Certificate::from_params(ca_params)?;

    // Generate client certificate signed by CA
    let mut client_params = CertificateParams::new(vec![client_subject.to_string()]);
    client_params.is_ca = IsCa::NoCa;

    let _client_key_pair = KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let client_cert = Certificate::from_params(client_params)?;
    let client_cert_signed = client_cert.serialize_pem_with_signer(&ca_cert)?;

    let ca_bundle = TestCertBundle {
        cert_pem: ca_cert.serialize_pem()?,
        key_pem: ca_cert.serialize_private_key_pem(),
        ca_pem: ca_cert.serialize_pem()?,
    };

    let client_bundle = TestCertBundle {
        cert_pem: client_cert_signed,
        key_pem: client_cert.serialize_private_key_pem(),
        ca_pem: ca_cert.serialize_pem()?,
    };

    Ok((ca_bundle, client_bundle))
}

/// Find a free port on localhost
pub fn find_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Write certificate bundle to files in a directory
pub fn write_cert_bundle(
    bundle: &TestCertBundle,
    dir: &Path,
    name: &str,
) -> Result<(String, String, String)> {
    let cert_path = dir.join(format!("{name}-cert.pem"));
    let key_path = dir.join(format!("{name}-key.pem"));
    let ca_path = dir.join(format!("{name}-ca.pem"));

    let mut cert_file = File::create(&cert_path)?;
    cert_file.write_all(bundle.cert_pem.as_bytes())?;

    let mut key_file = File::create(&key_path)?;
    key_file.write_all(bundle.key_pem.as_bytes())?;

    let mut ca_file = File::create(&ca_path)?;
    ca_file.write_all(bundle.ca_pem.as_bytes())?;

    Ok((
        cert_path.to_string_lossy().to_string(),
        key_path.to_string_lossy().to_string(),
        ca_path.to_string_lossy().to_string(),
    ))
}

/// Create a TLS client configuration that trusts a specific certificate
pub fn create_test_client_config(
    ca_pem: &str,
    client_cert: Option<(&str, &str)>,
) -> Result<ClientConfig> {
    let ca_cert_der: Vec<CertificateDer> =
        certs(&mut BufReader::new(ca_pem.as_bytes())).collect::<Result<Vec<_>, _>>()?;

    let mut root_store = rustls::RootCertStore::empty();
    for cert in ca_cert_der {
        root_store.add(cert)?;
    }

    let config_builder = ClientConfig::builder().with_root_certificates(root_store);

    let config = if let Some((cert_pem, key_pem)) = client_cert {
        let client_cert_der: Vec<CertificateDer> =
            certs(&mut BufReader::new(cert_pem.as_bytes())).collect::<Result<Vec<_>, _>>()?;

        let client_key_der = private_key(&mut BufReader::new(key_pem.as_bytes()))?
            .ok_or_else(|| anyhow::anyhow!("No private key found"))?;

        config_builder.with_client_auth_cert(client_cert_der, client_key_der)?
    } else {
        config_builder.with_no_client_auth()
    };

    Ok(config)
}

/// Mock plaintext backend server that echoes data
pub async fn run_mock_plaintext_backend(port: u16) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;

    loop {
        let (socket, _) = listener.accept().await?;

        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let mut socket = socket;
            let mut buffer = [0u8; 1024];

            loop {
                match socket.read(&mut buffer).await {
                    Ok(0) => break, // Connection closed
                    Ok(n) => {
                        if socket.write_all(&buffer[..n]).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }
}

/// Create a TOML configuration file for testing
pub fn create_test_config(
    temp_dir: &TempDir,
    proxy_bind_port: u16,
    backend_port: u16,
    server_cert_path: &str,
    server_key_path: &str,
    mtls: bool,
    client_ca_path: Option<&str>,
) -> Result<String> {
    let config_content = format!(
        r#"log_level = "debug"

[[proxy]]
[proxy.listener]
bind_address = "127.0.0.1:{}"
server_cert = "{}"
server_key = "{}"
mtls = {}
{}

[proxy.backend]
address = "127.0.0.1:{}"
"#,
        proxy_bind_port,
        server_cert_path,
        server_key_path,
        mtls,
        if let Some(ca_path) = client_ca_path {
            format!(r#"client_ca = "{ca_path}""#)
        } else {
            String::new()
        },
        backend_port,
    );

    let config_path = temp_dir.path().join("pgtls.toml");
    let mut config_file = File::create(&config_path)?;
    config_file.write_all(config_content.as_bytes())?;

    Ok(config_path.to_string_lossy().to_string())
}

/// Wait for a process to start listening on a port
pub async fn wait_for_port(port: u16, timeout_secs: u64) -> Result<()> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    while start.elapsed() < timeout {
        match TcpStream::connect(format!("127.0.0.1:{port}")).await {
            Ok(_) => return Ok(()),
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }

    Err(anyhow::anyhow!(
        "Timeout waiting for port {} to be available",
        port
    ))
}
