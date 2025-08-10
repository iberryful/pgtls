# **Task 005: Integration Testing**

## **1. Objective**

The goal of this task is to build a suite of end-to-end integration tests that run the compiled `pgtls` binary. These tests will validate the application as a whole, including configuration parsing, CLI arguments, and the proxying logic, using real network sockets.

## **2. Implementation Steps**

### **2.1. Create `tests/` directory**

Rust's convention is to place integration tests in the `tests/` directory at the root of the crate. Create this directory.

### **2.2. Create `tests/common/mod.rs`**

We will need some shared test utilities. Create a `common` module to house them. This file will contain helper functions for:
*   Generating self-signed certificates for testing. The `rcgen` crate is excellent for this.
*   Setting up mock client and server tasks.
*   Finding a free port to bind to.

### **2.3. Create `tests/proxy_tests.rs`**

This file will contain the integration tests.

### **2.4. Test Case 1: TLS-to-Plaintext**

*   **Setup**:
    1.  In the test, programmatically generate a server certificate for the proxy to use.
    2.  Create a `pgtls.toml` configuration file in a temporary directory. This config will define one proxy route pointing to a plaintext backend.
    3.  Launch a mock plaintext backend server in a separate task that listens on a specific port, receives data, and echoes it back.
    4.  Launch the `pgtls` binary as a child process, providing it with the path to the temporary config file. Give it a moment to start up.
*   **Execution**:
    1.  In the main test task, act as a client.
    2.  Connect to the `pgtls` listener port using TLS (you'll need to create a `rustls::ClientConfig` that trusts the self-signed server cert).
    3.  Perform the `SSLRequest` handshake.
    4.  Send a known payload (e.g., `b"integration test 1"`) through the TLS stream.
    5.  Read the response from the stream.
*   **Assertions**:
    1.  Assert that the received response is identical to the sent payload (confirming the echo from the mock backend).
    2.  Assert that the `pgtls` process runs without errors and can be terminated gracefully.

### **2.5. Test Case 2: TLS-to-TLS**

*   **Setup**:
    1.  Generate two sets of certificates: one for the proxy's listener and one for the mock backend server.
    2.  Create a `pgtls.toml` config that defines a TLS-to-TLS route. The `backend.root_ca` will point to the CA cert for the mock backend.
    3.  Launch a mock **TLS** backend server. This server will use its generated certificate and key, perform a TLS handshake, and then echo data.
    4.  Launch the `pgtls` binary as a child process.
*   **Execution**:
    1.  Act as a client and connect to the `pgtls` listener with TLS.
    2.  Perform the `SSLRequest` handshake.
    3.  Send a known payload.
    4.  Read the response.
*   **Assertions**:
    1.  Assert that the received response is identical to the sent payload.
    2.  Assert that the `pgtls` process runs without errors.

### **2.6. Test Case 3: mTLS**

*   **Setup**:
    1.  Generate three sets of certificates: one for the proxy listener, one for the mock backend, and one for the mock **client**.
    2.  Create a `pgtls.toml` config that enables `mtls` on the listener and points `client_ca` to the client's CA.
    3.  Launch the `pgtls` binary.
*   **Execution**:
    1.  Act as a client. This time, the client's `rustls::ClientConfig` must be configured with its own client certificate and key.
    2.  Connect and send/receive data.
*   **Assertions**:
    1.  Assert that the connection and data transfer are successful.
*   **Bonus Test**:
    1.  Try to connect as a client *without* providing a client certificate.
    2.  Assert that the TLS handshake fails and the connection is rejected by `pgtls`.

These integration tests provide the highest level of confidence that the application works as expected in real-world scenarios.
