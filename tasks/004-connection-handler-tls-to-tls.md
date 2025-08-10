# **Task 004: Connection Handler (TLS-to-TLS)**

## **1. Objective**

This task extends the `handle_connection` function to support the TLS-to-TLS proxying scenario. This involves initiating a TLS handshake with the backend server after connecting to it.

## **2. Implementation Steps**

### **2.1. Modify `handle_connection` in `src/proxy.rs`**

The existing `handle_connection` function will be updated to include logic for the `backend.tls_enabled = true` case.

```rust
// src/proxy.rs
// ... imports ...
use tokio_rustls::{TlsAcceptor, TlsConnector};

async fn handle_connection(
    mut client_socket: tokio::net::TcpStream,
    proxy_config: config::Proxy,
    server_config: Arc<rustls::ServerConfig>,
    // We will need a client_config for the backend connection
    client_config: Arc<rustls::ClientConfig>,
) -> anyhow::Result<()> {
    let mut buffer = [0u8; 8];
    let request_type = protocol::parse_request(&mut client_socket, &mut buffer).await?;

    match request_type {
        RequestType::Ssl => {
            client_socket.write_all(b"S").await?;

            let acceptor = TlsAcceptor::from(server_config);
            let client_tls_stream = acceptor.accept(client_socket).await?;

            // NEW LOGIC STARTS HERE
            let backend_stream: Box<dyn tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> =
                if proxy_config.backend.tls_enabled {
                    // Connect to the backend
                    let mut backend_socket = tokio::net::TcpStream::connect(&proxy_config.backend.address).await?;

                    // Perform the SSLRequest handshake with the backend
                    backend_socket.write_all(&[0, 0, 0, 8, 4, 210, 22, 47]).await?;
                    let mut response = [0u8; 1];
                    backend_socket.read_exact(&mut response).await?;
                    if response[0] != b'S' {
                        anyhow::bail!("Backend does not support TLS");
                    }

                    // Perform TLS handshake with the backend
                    let connector = TlsConnector::from(client_config);
                    let server_name = proxy_config.backend.address.split(':').next().unwrap().try_into()?;
                    let backend_tls_stream = connector.connect(server_name, backend_socket).await?;
                    Box::new(backend_tls_stream)
                } else {
                    // Existing plaintext logic
                    let backend_socket = tokio::net::TcpStream::connect(&proxy_config.backend.address).await?;
                    Box::new(backend_socket)
                };
            // NEW LOGIC ENDS HERE

            proxy_streams(client_tls_stream, backend_stream).await?;
        }
        // ...
    }
    Ok(())
}
```
*Note: We use `Box<dyn ...>` to handle the fact that the backend stream can be one of two different types (`TlsStream` or `TcpStream`).*

### **2.2. Update `run_proxy`**

The `run_proxy` function will need to be updated to create and pass the `rustls::ClientConfig`.

```rust
// src/proxy.rs
pub async fn run_proxy(proxy_config: config::Proxy) -> anyhow::Result<()> {
    // Stubbed TLS context creation
    let server_config = Arc::new(create_stub_server_config()?);
    let client_config = Arc::new(create_stub_client_config()?); // New

    let listener = TcpListener::bind(&proxy_config.listener.bind_address).await?;

    loop {
        let (client_socket, _) = listener.accept().await?;
        let proxy_config = proxy_config.clone();
        let server_config = server_config.clone();
        let client_config = client_config.clone(); // New

        tokio::spawn(async move {
            if let Err(e) = handle_connection(client_socket, proxy_config, server_config, client_config).await {
                eprintln!("Error handling connection: {}", e);
            }
        });
    }
}
```

## **3. Testing Strategy (TDD)**

We will add a new test case to `src/proxy.rs` to cover the TLS-to-TLS scenario.

### **3.1. Create New Test Case**

1.  **`test_tls_to_tls_proxy`**:
    *   **Setup**:
        *   Create a mock client task (same as in Task 003).
        *   Create a mock **TLS** backend server task. This task is more complex:
            1.  Listen on a known port.
            2.  Accept a connection.
            3.  Wait to receive the `SSLRequest` bytes.
            4.  Send the `'S'` byte.
            5.  (Using a mock TLS library or stub) Complete a "TLS handshake" as a server.
            6.  Wait to receive the "client data" over the mock TLS stream.
            7.  Send the "backend data" over the mock TLS stream.
    *   **Execution**:
        *   Start the mock TLS backend server.
        *   Call `handle_connection` with a configuration where `backend.tls_enabled = true` and it points to the mock backend.
    *   **Assertions**:
        *   Assert that the mock backend received the exact "client data".
        *   Assert that the mock client received the exact "backend data".
        *   Assert that all tasks complete without errors.

This test validates the entire data flow for the TLS-to-TLS scenario, including the proxy's ability to act as a TLS client.
