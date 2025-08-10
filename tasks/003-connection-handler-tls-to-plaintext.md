# **Task 003: Connection Handler (TLS-to-Plaintext)**

## **1. Objective**

This task is to implement the core data-plane logic for a common use case: accepting a TLS-encrypted connection from a client and proxying it to a backend PostgreSQL server over a plaintext TCP connection.

## **2. Implementation Steps**

### **2.1. Create `src/proxy.rs`**

Create a new module `src/proxy.rs` that will contain the connection handling logic.

### **2.2. Create `run_proxy` function**

This will be the main entry point for a proxy route. It will take a `config::Proxy` and be responsible for setting up the listener and accepting connections.

```rust
// src/proxy.rs
use crate::config;
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn run_proxy(proxy_config: config::Proxy) -> anyhow::Result<()> {
    // For now, we will stub out the TLS context creation.
    // In a later task, this will be properly implemented.
    let server_config = Arc::new(create_stub_server_config()?);

    let listener = TcpListener::bind(&proxy_config.listener.bind_address).await?;

    loop {
        let (client_socket, _) = listener.accept().await?;
        let proxy_config = proxy_config.clone(); // Not ideal, will be improved
        let server_config = server_config.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(client_socket, proxy_config, server_config).await {
                // Basic logging for now
                eprintln!("Error handling connection: {}", e);
            }
        });
    }
}
```

### **2.3. Implement `handle_connection`**

This function will contain the state machine for a single connection.

```rust
// src/proxy.rs
// ... imports ...
use crate::protocol::{self, RequestType};
use tokio::io::AsyncWriteExt;
use tokio_rustls::TlsAcceptor;

async fn handle_connection(
    mut client_socket: tokio::net::TcpStream,
    proxy_config: config::Proxy,
    server_config: Arc<rustls::ServerConfig>,
) -> anyhow::Result<()> {
    let mut buffer = [0u8; 8];
    let request_type = protocol::parse_request(&mut client_socket, &mut buffer).await?;

    match request_type {
        RequestType::Ssl => {
            // It's an SSLRequest, respond with 'S'
            client_socket.write_all(b"S").await?;

            // Perform TLS handshake with the client
            let acceptor = TlsAcceptor::from(server_config);
            let client_tls_stream = acceptor.accept(client_socket).await?;

            // Connect to the plaintext backend
            let backend_socket = tokio::net::TcpStream::connect(&proxy_config.backend.address).await?;

            // Relay data between the two streams
            proxy_streams(client_tls_stream, backend_socket).await?;
        }
        RequestType::Startup(initial_bytes) => {
            // This is a plaintext request. For this task, we will simply
            // log a warning and close the connection, as we are focusing
            // on the TLS path.
            eprintln!("Received plaintext request, which is not supported in this path.");
        }
    }
    Ok(())
}
```

### **2.4. Implement `proxy_streams`**

Create a helper function that uses `tokio::io::copy_bidirectional` to relay data between the client and backend streams.

```rust
// src/proxy.rs
use tokio::io;

async fn proxy_streams<A, B>(mut client: A, mut backend: B) -> anyhow::Result<()>
where
    A: io::AsyncRead + io::AsyncWrite + Unpin,
    B: io::AsyncRead + io::AsyncWrite + Unpin,
{
    let (mut client_reader, mut client_writer) = io::split(client);
    let (mut backend_reader, mut backend_writer) = io::split(backend);

    tokio::select! {
        res = io::copy(&mut client_reader, &mut backend_writer) => {
            res?;
        },
        res = io::copy(&mut backend_reader, &mut client_writer) => {
            res?;
        },
    }
    Ok(())
}
```

## **3. Testing Strategy (TDD)**

Testing this requires mocking both the client and the backend server.

### **3.1. Create Test Cases in `src/proxy.rs`**

1.  **`test_tls_to_plaintext_proxy`**:
    *   **Setup**:
        *   Create a mock client task. This task will:
            1.  Send the `SSLRequest` bytes.
            2.  Wait to receive the `'S'` byte.
            3.  (Using a mock TLS library or stub) Complete a "TLS handshake".
            4.  Send some known "client data" (e.g., `b"hello from client"`).
            5.  Wait to receive some "backend data" (e.g., `b"hello from backend"`).
        *   Create a mock backend server task. This task will:
            1.  Listen on a known port.
            2.  Accept a connection.
            3.  Wait to receive the "client data".
            4.  Send the "backend data".
    *   **Execution**:
        *   Start the mock backend server.
        *   Call `handle_connection` with the mock client stream and a configuration pointing to the mock backend.
    *   **Assertions**:
        *   Assert that the mock backend received the exact "client data".
        *   Assert that the mock client received the exact "backend data".
        *   Assert that all tasks complete without errors.

This test validates the entire data flow for the TLS-to-plaintext scenario.
