# pgtls: A Protocol-Aware TLS Proxy for PostgreSQL

[![Build Status](https://github.com/tyrchen/pgtls/workflows/build/badge.svg)](https://github.com/tyrchen/pgtls/actions)

`pgtls` is a high-performance, protocol-aware TLS termination proxy for PostgreSQL, written in Rust.

It is designed to solve a specific, well-known problem: standard TLS proxies (like Nginx, HAProxy, or cloud load balancers) cannot correctly handle PostgreSQL's TLS negotiation, because PostgreSQL uses a `STARTTLS`-like mechanism within its own protocol instead of initiating a TLS handshake immediately after the TCP connection is established.

`pgtls` acts as a "man-in-the-middle" that understands the PostgreSQL wire protocol, correctly handles the `SSLRequest` negotiation, and then establishes two separate TLS sessions: one with the client and one with the backend PostgreSQL server.

## Features

*   **PostgreSQL Protocol-Aware**: Correctly handles the `SSLRequest` handshake process.
*   **TLS Termination**: Adds a TLS security layer for clients to connect to.
*   **Flexible Backend Connection**: Can connect to the backend PostgreSQL server using either a new TLS connection (TLS Re-origination) or a standard plaintext connection. This allows you to secure databases that are not themselves configured for TLS.
*   **Secure by Default**: Built with Rust, `rustls`, and `tokio` for a modern, safe, and asynchronous architecture.
*   **Mutual TLS (mTLS) Support**: Can be configured to require and verify client certificates, and can present its own client certificate to the backend.
*   **Configuration via TOML**: Simple and clear configuration.

## How It Works

```mermaid
graph TD
    Client[PostgreSQL Client] -- "TLS Connection" --> PGTLS[pgtls Proxy]

    subgraph "Backend Connection"
        direction LR
        PGTLS -- "TLS (Optional)" --> PGServer[PostgreSQL Server]
    end
```

For a detailed architectural overview and protocol specifications, please see the documents in the `/specs` directory.

## Getting Started

*(This section will be updated once the first release is available.)*

1.  **Installation**:
    ```bash
    # Coming soon
    ```

2.  **Configuration**: Create a `pgtls.toml` file. See [`specs/004-configuration.md`](specs/004-configuration.md) for a full reference.

    A single `pgtls` instance can manage multiple proxy routes. Each route maps one listener to one backend.

    ```toml
    # pgtls.toml
    log_level = "info"

    # Proxy route #1: Secure a TLS-enabled backend and require mTLS from clients.
    [[proxy]]
      [proxy.listener]
      bind_address = "0.0.0.0:6432"
      server_cert = "/etc/pgtls/certs/proxy-server.pem"
      server_key = "/etc/pgtls/certs/proxy-server.key"
      mtls = true
      client_ca = "/etc/pgtls/certs/client-ca.pem"

      [proxy.backend]
      address = "db.example.com:5432"
      tls_enabled = true
      root_ca = "/etc/pgtls/certs/backend-ca.pem"

    # Proxy route #2: Add a TLS layer to a plaintext-only backend.
    [[proxy]]
      [proxy.listener]
      bind_address = "127.0.0.1:6433"
      server_cert = "/etc/pgtls/certs/internal-proxy.pem"
      server_key = "/etc/pgtls/certs/internal-proxy.key"

      [proxy.backend]
      address = "10.0.1.50:5432"
      tls_enabled = false
    ```

3.  **Run the proxy**:
    ```bash
    pgtls --config pgtls.toml
    ```

## License

This project is distributed under the terms of the MIT license. See [LICENSE.md](LICENSE.md) for details.

Copyright 2025 Tyr Chen
