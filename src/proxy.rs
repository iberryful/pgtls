use crate::{
    config,
    protocol::{self, RequestType},
};
use anyhow::{Result, anyhow};
use rustls::ServerConfig;
use rustls_pemfile::{certs, private_key};
use rustls_pki_types::CertificateDer;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

pub async fn run_proxy(proxy_config: config::Proxy) -> Result<()> {
    tracing::info!("Creating TLS server configuration for proxy");
    let server_config = Arc::new(create_server_config(&proxy_config.listener)?);

    tracing::info!(
        "Starting proxy listener on {}",
        proxy_config.listener.bind_address
    );
    let listener = TcpListener::bind(&proxy_config.listener.bind_address).await?;

    tracing::info!("Proxy ready to accept connections (TLS-to-plaintext mode)");
    loop {
        let (client_socket, client_addr) = listener.accept().await?;
        tracing::debug!("Accepted connection from {}", client_addr);

        let proxy_config = proxy_config.clone();
        let server_config = server_config.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(client_socket, proxy_config, server_config).await {
                tracing::error!("Error handling connection from {}: {}", client_addr, e);
            } else {
                tracing::debug!("Connection from {} completed successfully", client_addr);
            }
        });
    }
}

async fn handle_connection(
    mut client_socket: TcpStream,
    proxy_config: config::Proxy,
    server_config: Arc<ServerConfig>,
) -> Result<()> {
    let mut buffer = [0u8; 8];
    let request_type = protocol::parse_request(&mut client_socket, &mut buffer).await?;

    match request_type {
        RequestType::Ssl => {
            // It's an SSLRequest, respond with 'S'
            client_socket.write_all(b"S").await?;

            // Perform TLS handshake with the client
            let acceptor = TlsAcceptor::from(server_config);
            let client_tls_stream = acceptor.accept(client_socket).await?;

            // Connect to backend (plaintext only)
            let backend_socket = TcpStream::connect(&proxy_config.backend.address).await?;

            // Relay data between TLS client and plaintext backend
            proxy_streams(client_tls_stream, backend_socket).await?;
        }
        RequestType::Startup(initial_bytes) => {
            // This is a plaintext request - connect to plaintext backend
            let mut backend_socket = TcpStream::connect(&proxy_config.backend.address).await?;

            // Replay the initial startup bytes to the backend
            backend_socket.write_all(initial_bytes).await?;

            // Relay data between plaintext streams
            proxy_streams(client_socket, backend_socket).await?;
        }
    }
    Ok(())
}

async fn proxy_streams<A, B>(client: A, backend: B) -> Result<()>
where
    A: io::AsyncRead + io::AsyncWrite + Unpin,
    B: io::AsyncRead + io::AsyncWrite + Unpin,
{
    let (mut client_reader, mut client_writer) = io::split(client);
    let (mut backend_reader, mut backend_writer) = io::split(backend);

    let client_to_backend = async {
        let result = io::copy(&mut client_reader, &mut backend_writer).await;
        // Attempt graceful shutdown of backend writer
        let _ = backend_writer.shutdown().await;
        result
    };

    let backend_to_client = async {
        let result = io::copy(&mut backend_reader, &mut client_writer).await;
        // Attempt graceful shutdown of client writer
        let _ = client_writer.shutdown().await;
        result
    };

    tokio::select! {
        res = client_to_backend => {
            res?;
        },
        res = backend_to_client => {
            res?;
        },
    }
    Ok(())
}

fn create_server_config(listener_config: &config::Listener) -> Result<ServerConfig> {
    let cert_file = File::open(&listener_config.server_cert)?;
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain: Vec<CertificateDer> = certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?;

    let key_file = File::open(&listener_config.server_key)?;
    let mut key_reader = BufReader::new(key_file);
    let private_key =
        private_key(&mut key_reader)?.ok_or_else(|| anyhow!("No private key found in key file"))?;

    let config = if listener_config.mtls {
        // mTLS enabled - require client certificates
        if let Some(client_ca_path) = &listener_config.client_ca {
            let ca_file = File::open(client_ca_path)?;
            let mut ca_reader = BufReader::new(ca_file);
            let ca_certs: Vec<CertificateDer> =
                certs(&mut ca_reader).collect::<Result<Vec<_>, _>>()?;

            let mut client_auth_roots = rustls::RootCertStore::empty();
            for cert in ca_certs {
                client_auth_roots.add(cert)?;
            }

            let client_cert_verifier =
                rustls::server::WebPkiClientVerifier::builder(client_auth_roots.into()).build()?;

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

// Stub function for basic testing - creates a self-signed cert in memory
#[cfg(test)]
#[allow(dead_code)]
fn create_stub_server_config() -> Result<ServerConfig> {
    // Create a minimal self-signed certificate for testing
    let cert_bytes = include_bytes!("../fixtures/test-cert.pem");
    let key_bytes = include_bytes!("../fixtures/test-key.pem");

    let cert_chain: Vec<CertificateDer> =
        certs(&mut BufReader::new(&cert_bytes[..])).collect::<Result<Vec<_>, _>>()?;

    let private_key = private_key(&mut BufReader::new(&key_bytes[..]))?
        .ok_or_else(|| anyhow!("No private key found in test key file"))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Backend, Listener, Proxy};

    // Helper function to create a test proxy configuration
    #[allow(dead_code)]
    fn create_test_proxy_config(backend_port: u16) -> Proxy {
        Proxy {
            listener: Listener {
                bind_address: "127.0.0.1:0".to_string(), // Let OS choose port
                server_cert: "fixtures/test-cert.pem".to_string(),
                server_key: "fixtures/test-key.pem".to_string(),
                mtls: false,
                client_ca: None,
            },
            backend: Backend {
                address: format!("127.0.0.1:{backend_port}"),
            },
        }
    }

    #[tokio::test]
    async fn test_handle_connection_ssl_request() {
        // Create a mock backend server
        let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_addr = backend_listener.local_addr().unwrap();

        // Spawn backend server task
        let _backend_task = tokio::spawn(async move {
            let (mut backend_stream, _) = backend_listener.accept().await.unwrap();

            // Read data from proxy
            let mut buffer = [0u8; 1024];
            backend_stream.readable().await.unwrap();
            let bytes_read = backend_stream.try_read(&mut buffer).unwrap();

            // Echo back the data
            backend_stream
                .write_all(&buffer[..bytes_read])
                .await
                .unwrap();
        });

        // Create proxy config pointing to our mock backend
        let proxy_config = Proxy {
            listener: Listener {
                bind_address: "127.0.0.1:0".to_string(),
                server_cert: "fixtures/test-cert.pem".to_string(),
                server_key: "fixtures/test-key.pem".to_string(),
                mtls: false,
                client_ca: None,
            },
            backend: Backend {
                address: backend_addr.to_string(),
            },
        };

        // This test would require actual TLS certificates and a more complex setup
        // For now, we'll test the basic structure and logic paths

        // Test that we can create a server config with our test certificates
        let result = create_server_config(&proxy_config.listener);
        // We expect this to succeed now with proper certificates
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_proxy_streams_basic() {
        // This test just verifies the structure compiles
        // For a real test, we'd need actual bidirectional streams
    }

    #[tokio::test]
    async fn test_handle_connection_startup_message() {
        // This test just verifies the structure compiles
        // In a real implementation, we'd test the actual StartupMessage handling
    }

    #[tokio::test]
    async fn test_create_server_config_missing_files() {
        let listener_config = Listener {
            bind_address: "127.0.0.1:6432".to_string(),
            server_cert: "/nonexistent/cert.pem".to_string(),
            server_key: "/nonexistent/key.pem".to_string(),
            mtls: false,
            client_ca: None,
        };

        let result = create_server_config(&listener_config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No such file or directory")
        );
    }
}
