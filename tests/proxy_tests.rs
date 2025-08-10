mod common;

use anyhow::Result;
use common::*;
use rustls_pki_types::ServerName;
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

#[tokio::test]
async fn test_tls_to_plaintext_integration() -> Result<()> {
    // Find free ports for proxy and backend
    let proxy_port = find_free_port()?;
    let backend_port = find_free_port()?;

    // Generate test certificate for proxy
    let proxy_cert = generate_test_certificate("localhost")?;

    // Create temporary directory for certificates and config
    let temp_dir = TempDir::new()?;
    let (proxy_cert_path, proxy_key_path, _proxy_ca_path) =
        write_cert_bundle(&proxy_cert, temp_dir.path(), "proxy")?;

    // Create configuration file
    let config_path = create_test_config(
        &temp_dir,
        proxy_port,
        backend_port,
        &proxy_cert_path,
        &proxy_key_path,
        false, // mtls = false
        None,  // no client CA
    )?;

    // Start mock plaintext backend
    let backend_task = tokio::spawn(async move { run_mock_plaintext_backend(backend_port).await });

    // Give backend time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start pgtls proxy
    let mut proxy_process = std::process::Command::new("./target/debug/pgtls")
        .args([&config_path])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    // Wait for proxy to start
    wait_for_port(proxy_port, 5).await?;

    // Test the proxy
    let test_result = async {
        // Give proxy extra time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create TLS client configuration that trusts our test certificate
        let client_config = create_test_client_config(&proxy_cert.ca_pem, None)?;
        let connector = TlsConnector::from(std::sync::Arc::new(client_config));

        // Connect to proxy
        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy_port}")).await?;

        // Perform SSLRequest handshake
        let mut stream = stream;
        stream.write_all(&[0, 0, 0, 8, 4, 210, 22, 47]).await?; // SSLRequest

        let mut response = [0u8; 1];
        stream.read_exact(&mut response).await?;
        assert_eq!(response[0], b'S', "Expected 'S' response to SSLRequest");

        // Perform TLS handshake
        let server_name = ServerName::try_from("localhost")?;
        let mut tls_stream = connector.connect(server_name, stream).await?;

        // Send test data
        let test_payload = b"integration test tls-to-plaintext";
        tls_stream.write_all(test_payload).await?;

        // Read response with timeout
        let mut buffer = vec![0u8; test_payload.len()];
        timeout(Duration::from_secs(2), tls_stream.read_exact(&mut buffer)).await??;

        // Verify echo
        assert_eq!(&buffer, test_payload, "Data was not echoed correctly");

        // Gracefully close the TLS stream
        tls_stream.shutdown().await.ok();

        Ok::<_, anyhow::Error>(())
    }
    .await;

    // Clean up
    proxy_process.kill().ok();
    backend_task.abort();

    test_result?;
    Ok(())
}

#[tokio::test]
async fn test_mtls_integration() -> Result<()> {
    // Find free ports
    let proxy_port = find_free_port()?;
    let backend_port = find_free_port()?;

    // Generate certificates - CA and client cert signed by CA
    let proxy_cert = generate_test_certificate("localhost")?;
    let (ca_bundle, client_bundle) = generate_ca_and_client_certificate("test-ca", "test-client")?;

    // Create temporary directory
    let temp_dir = TempDir::new()?;
    let (proxy_cert_path, proxy_key_path, _proxy_ca_path) =
        write_cert_bundle(&proxy_cert, temp_dir.path(), "proxy")?;
    let (_ca_cert_path, _ca_key_path, ca_ca_path) =
        write_cert_bundle(&ca_bundle, temp_dir.path(), "ca")?;
    let (_client_cert_path, _client_key_path, _client_ca_path) =
        write_cert_bundle(&client_bundle, temp_dir.path(), "client")?;

    // Create configuration file with mTLS enabled
    let config_path = create_test_config(
        &temp_dir,
        proxy_port,
        backend_port,
        &proxy_cert_path,
        &proxy_key_path,
        true,              // mtls = true
        Some(&ca_ca_path), // client CA
    )?;

    // Start mock plaintext backend
    let backend_task = tokio::spawn(async move { run_mock_plaintext_backend(backend_port).await });

    // Give backend time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start pgtls proxy
    let mut proxy_process = std::process::Command::new("./target/debug/pgtls")
        .args([&config_path])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    // Wait for proxy to start
    wait_for_port(proxy_port, 5).await?;

    // Test 1: Successful connection with client certificate
    let test_with_cert_result = async {
        // Create TLS client configuration with client certificate
        let client_config = create_test_client_config(
            &proxy_cert.ca_pem,
            Some((&client_bundle.cert_pem, &client_bundle.key_pem)),
        )?;
        let connector = TlsConnector::from(std::sync::Arc::new(client_config));

        // Connect to proxy
        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy_port}")).await?;

        // Perform SSLRequest handshake
        let mut stream = stream;
        stream.write_all(&[0, 0, 0, 8, 4, 210, 22, 47]).await?; // SSLRequest

        let mut response = [0u8; 1];
        stream.read_exact(&mut response).await?;
        assert_eq!(response[0], b'S', "Expected 'S' response to SSLRequest");

        // Perform TLS handshake with client cert
        let server_name = ServerName::try_from("localhost")?;
        let mut tls_stream = connector.connect(server_name, stream).await?;

        // Send test data
        let test_payload = b"integration test mtls with cert";
        tls_stream.write_all(test_payload).await?;

        // Read response
        let mut buffer = vec![0u8; test_payload.len()];
        tls_stream.read_exact(&mut buffer).await?;

        // Verify echo
        assert_eq!(
            &buffer, test_payload,
            "Data was not echoed correctly with mTLS"
        );

        Ok::<_, anyhow::Error>(())
    }
    .await;

    // Test 2: Connection should fail without client certificate
    let test_without_cert_result = async {
        // Create TLS client configuration WITHOUT client certificate
        let client_config = create_test_client_config(&proxy_cert.ca_pem, None)?;
        let connector = TlsConnector::from(std::sync::Arc::new(client_config));
        // Connect to proxy
        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy_port}")).await?;
        // Perform SSLRequest handshake
        let mut stream = stream;
        stream.write_all(&[0, 0, 0, 8, 4, 210, 22, 47]).await?; // SSLRequest
        let mut response = [0u8; 1];
        stream.read_exact(&mut response).await?;
        assert_eq!(response[0], b'S', "Expected 'S' response to SSLRequest");
        // TLS handshake should fail due to missing client certificate
        let server_name = ServerName::try_from("localhost")?;
        let handshake_result = connector.connect(server_name, stream).await;
        // Check if it failed for the right reason (certificate required)
        match handshake_result {
            Err(e) => {
                let error_msg = e.to_string();
                println!("TLS handshake failed as expected: {error_msg}");
                // Accept various certificate-related errors
                assert!(
                    error_msg.contains("certificate") ||
                    error_msg.contains("handshake") ||
                    error_msg.contains("peer") ||
                    error_msg.contains("alert") ||
                    error_msg.contains("required") ||
                    error_msg.contains("client"),
                    "Expected certificate-related error, got: {error_msg}"
                );
            }
            Ok(mut tls_stream) => {
                // If the handshake succeeded, let's see if we can actually send data
                // In some cases, the handshake might succeed but the connection gets closed later
                let test_payload = b"should fail";
                match tls_stream.write_all(test_payload).await {
                    Ok(_) => {
                        // If write succeeded, try to read - this might fail
                        let mut buffer = vec![0u8; test_payload.len()];
                        match tokio::time::timeout(
                            Duration::from_millis(500),
                            tls_stream.read_exact(&mut buffer)
                        ).await {
                            Ok(Ok(_)) => {
                                panic!("TLS handshake should fail without client certificate");
                            }
                            Ok(Err(e)) => {
                                println!("Connection failed during read as expected: {e}");
                                // This is acceptable - connection was rejected after handshake
                            }
                            Err(_) => {
                                println!("Connection timed out during read - this is expected without client cert");
                                // Timeout is also acceptable
                            }
                        }
                    }
                    Err(e) => {
                        println!("Connection failed during write as expected: {e}");
                        // This is acceptable - connection was rejected
                    }
                }
            }
        }

        Ok::<_, anyhow::Error>(())
    }.await;

    // Clean up
    proxy_process.kill().ok();
    backend_task.abort();

    test_with_cert_result?;
    test_without_cert_result?;
    Ok(())
}
