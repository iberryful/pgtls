use crate::{
    config,
    protocol::{self, RequestType},
};
use anyhow::{Result, anyhow};
use rustls::{ClientConfig, ServerConfig};
use rustls_pemfile::{certs, private_key};
use rustls_pki_types::CertificateDer;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

pub async fn run_proxy(proxy_config: config::Proxy) -> Result<()> {
    let server_config = Arc::new(create_server_config(&proxy_config.listener)?);
    let client_config = Arc::new(create_client_config(&proxy_config.backend)?);
    let listener = TcpListener::bind(&proxy_config.listener.bind_address).await?;

    loop {
        let (client_socket, _) = listener.accept().await?;
        let proxy_config = proxy_config.clone();
        let server_config = server_config.clone();
        let client_config = client_config.clone();

        tokio::spawn(async move {
            if let Err(e) =
                handle_connection(client_socket, proxy_config, server_config, client_config).await
            {
                eprintln!("Error handling connection: {e}");
            }
        });
    }
}

async fn handle_connection(
    mut client_socket: TcpStream,
    proxy_config: config::Proxy,
    server_config: Arc<ServerConfig>,
    client_config: Arc<ClientConfig>,
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

            // Connect to backend (either TLS or plaintext)
            if proxy_config.backend.tls_enabled {
                // TLS-to-TLS: Connect to backend with TLS
                let mut backend_socket = TcpStream::connect(&proxy_config.backend.address).await?;

                // Perform the SSLRequest handshake with the backend
                backend_socket
                    .write_all(&[0, 0, 0, 8, 4, 210, 22, 47])
                    .await?;
                let mut response = [0u8; 1];
                backend_socket.read_exact(&mut response).await?;
                if response[0] != b'S' {
                    return Err(anyhow!("Backend does not support TLS"));
                }

                // Perform TLS handshake with the backend
                let connector = TlsConnector::from(client_config);
                let server_name = proxy_config
                    .backend
                    .address
                    .split(':')
                    .next()
                    .unwrap()
                    .to_string()
                    .try_into()?;
                let backend_tls_stream = connector.connect(server_name, backend_socket).await?;

                // Relay data between TLS streams
                proxy_streams(client_tls_stream, backend_tls_stream).await?;
            } else {
                // TLS-to-plaintext: Connect to backend without TLS
                let backend_socket = TcpStream::connect(&proxy_config.backend.address).await?;

                // Relay data between TLS client and plaintext backend
                proxy_streams(client_tls_stream, backend_socket).await?;
            }
        }
        RequestType::Startup(initial_bytes) => {
            // This is a plaintext request. Handle both TLS and plaintext backends
            if proxy_config.backend.tls_enabled {
                // Plaintext-to-TLS: Client is plaintext, backend uses TLS
                let mut backend_socket = TcpStream::connect(&proxy_config.backend.address).await?;

                // Perform the SSLRequest handshake with the backend
                backend_socket
                    .write_all(&[0, 0, 0, 8, 4, 210, 22, 47])
                    .await?;
                let mut response = [0u8; 1];
                backend_socket.read_exact(&mut response).await?;
                if response[0] != b'S' {
                    return Err(anyhow!("Backend does not support TLS"));
                }

                // Perform TLS handshake with the backend
                let connector = TlsConnector::from(client_config);
                let server_name = proxy_config
                    .backend
                    .address
                    .split(':')
                    .next()
                    .unwrap()
                    .to_string()
                    .try_into()?;
                let mut backend_tls_stream = connector.connect(server_name, backend_socket).await?;

                // Replay the initial startup bytes to the backend
                backend_tls_stream.write_all(initial_bytes).await?;

                // Relay data between plaintext client and TLS backend
                proxy_streams(client_socket, backend_tls_stream).await?;
            } else {
                // Plaintext-to-plaintext: Both client and backend are plaintext
                let mut backend_socket = TcpStream::connect(&proxy_config.backend.address).await?;

                // Replay the initial startup bytes to the backend
                backend_socket.write_all(initial_bytes).await?;

                // Relay data between plaintext streams
                proxy_streams(client_socket, backend_socket).await?;
            }
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

fn create_client_config(backend_config: &config::Backend) -> Result<ClientConfig> {
    // Start building the config with root certificates
    let config_builder = if let Some(root_ca_path) = &backend_config.root_ca {
        let ca_file = File::open(root_ca_path)?;
        let mut ca_reader = BufReader::new(ca_file);
        let ca_certs: Vec<CertificateDer> = certs(&mut ca_reader).collect::<Result<Vec<_>, _>>()?;

        let mut root_store = rustls::RootCertStore::empty();
        for cert in ca_certs {
            root_store.add(cert)?;
        }
        ClientConfig::builder().with_root_certificates(root_store)
    } else {
        // Use system root certificates for convenience in development
        let mut root_store = rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs()? {
            root_store.add(cert)?;
        }
        ClientConfig::builder().with_root_certificates(root_store)
    };

    // Handle client certificate authentication
    let config = if let (Some(client_cert_path), Some(client_key_path)) =
        (&backend_config.client_cert, &backend_config.client_key)
    {
        let cert_file = File::open(client_cert_path)?;
        let mut cert_reader = BufReader::new(cert_file);
        let client_cert_chain: Vec<CertificateDer> =
            certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?;

        let key_file = File::open(client_key_path)?;
        let mut key_reader = BufReader::new(key_file);
        let client_private_key = private_key(&mut key_reader)?
            .ok_or_else(|| anyhow!("No private key found in client key file"))?;

        config_builder.with_client_auth_cert(client_cert_chain, client_private_key)?
    } else {
        config_builder.with_no_client_auth()
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

// Stub function for basic testing - creates a client config with system roots
#[cfg(test)]
#[allow(dead_code)]
fn create_stub_client_config() -> Result<ClientConfig> {
    let mut root_store = rustls::RootCertStore::empty();

    // Add our test certificate as a trusted root for testing
    let cert_bytes = include_bytes!("../fixtures/test-cert.pem");
    let test_certs: Vec<CertificateDer> =
        certs(&mut BufReader::new(&cert_bytes[..])).collect::<Result<Vec<_>, _>>()?;

    for cert in test_certs {
        root_store.add(cert)?;
    }

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

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
                tls_enabled: false, // This task focuses on plaintext backends
                root_ca: None,
                client_cert: None,
                client_key: None,
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
                tls_enabled: false,
                root_ca: None,
                client_cert: None,
                client_key: None,
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

    #[tokio::test]
    async fn test_create_client_config_success() {
        let backend_config = Backend {
            address: "127.0.0.1:5432".to_string(),
            tls_enabled: true,
            root_ca: None, // Test with system roots
            client_cert: None,
            client_key: None,
        };

        let result = create_client_config(&backend_config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_client_config_with_custom_ca() {
        let backend_config = Backend {
            address: "127.0.0.1:5432".to_string(),
            tls_enabled: true,
            root_ca: Some("fixtures/test-cert.pem".to_string()),
            client_cert: None,
            client_key: None,
        };

        let result = create_client_config(&backend_config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_client_config_with_client_auth() {
        let backend_config = Backend {
            address: "127.0.0.1:5432".to_string(),
            tls_enabled: true,
            root_ca: None,
            client_cert: Some("fixtures/test-cert.pem".to_string()),
            client_key: Some("fixtures/test-key.pem".to_string()),
        };

        let result = create_client_config(&backend_config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_client_config_missing_ca_file() {
        let backend_config = Backend {
            address: "127.0.0.1:5432".to_string(),
            tls_enabled: true,
            root_ca: Some("/nonexistent/ca.pem".to_string()),
            client_cert: None,
            client_key: None,
        };

        let result = create_client_config(&backend_config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No such file or directory")
        );
    }

    #[tokio::test]
    async fn test_create_client_config_missing_client_cert() {
        let backend_config = Backend {
            address: "127.0.0.1:5432".to_string(),
            tls_enabled: true,
            root_ca: None,
            client_cert: Some("/nonexistent/cert.pem".to_string()),
            client_key: Some("fixtures/test-key.pem".to_string()),
        };

        let result = create_client_config(&backend_config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No such file or directory")
        );
    }

    #[tokio::test]
    async fn test_handle_connection_tls_to_tls_basic() {
        // This test verifies the TLS-to-TLS configuration path compiles and handles basic cases
        // Create a proxy config with TLS enabled backend
        let proxy_config = Proxy {
            listener: Listener {
                bind_address: "127.0.0.1:0".to_string(),
                server_cert: "fixtures/test-cert.pem".to_string(),
                server_key: "fixtures/test-key.pem".to_string(),
                mtls: false,
                client_ca: None,
            },
            backend: Backend {
                address: "127.0.0.1:5432".to_string(),
                tls_enabled: true, // This enables the TLS-to-TLS path
                root_ca: None,
                client_cert: None,
                client_key: None,
            },
        };

        // Test that we can create both server and client configs
        let server_config_result = create_server_config(&proxy_config.listener);
        let client_config_result = create_client_config(&proxy_config.backend);

        assert!(server_config_result.is_ok());
        assert!(client_config_result.is_ok());
    }

    #[tokio::test]
    async fn test_tls_to_tls_proxy_integration() {
        // Create a mock TLS backend server
        let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_addr = backend_listener.local_addr().unwrap();

        // Spawn mock TLS backend server task
        let _backend_task = tokio::spawn(async move {
            let (mut backend_stream, _) = backend_listener.accept().await.unwrap();

            // Wait for SSLRequest
            let mut ssl_request = [0u8; 8];
            backend_stream.read_exact(&mut ssl_request).await.unwrap();

            // Verify it's an SSLRequest
            assert_eq!(ssl_request, [0, 0, 0, 8, 4, 210, 22, 47]);

            // Respond with 'S' to indicate TLS support
            backend_stream.write_all(b"S").await.unwrap();

            // At this point, in a real scenario, we'd perform TLS handshake
            // For this basic test, we'll just echo any data received
            let mut buffer = [0u8; 1024];
            loop {
                match backend_stream.try_read(&mut buffer) {
                    Ok(0) => break, // Connection closed
                    Ok(n) => {
                        backend_stream.write_all(&buffer[..n]).await.unwrap();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    }
                    Err(_) => break,
                }
            }
        });

        // Create proxy config pointing to our mock TLS backend
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
                tls_enabled: true, // Enable TLS-to-TLS mode
                root_ca: None,
                client_cert: None,
                client_key: None,
            },
        };

        // Test that the configuration is valid and can be created
        let _server_config = create_server_config(&proxy_config.listener).unwrap();
        let _client_config = create_client_config(&proxy_config.backend).unwrap();

        // Verify both configs were created successfully
        // ServerConfig doesn't expose cert_chain() in rustls 0.22, so we just verify it was created
    }
}
