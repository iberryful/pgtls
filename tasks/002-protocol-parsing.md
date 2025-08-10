# **Task 002: Protocol Parsing**

## **1. Objective**

The goal of this task is to implement the logic that inspects the initial bytes of a client connection to determine whether it is a TLS request or a plaintext startup message. This is the core protocol-aware component of `pgtls`.

## **2. Implementation Steps**

### **2.1. Create `src/protocol.rs`**

Create a new module `src/protocol.rs` to contain the parsing logic.

### **2.2. Define `RequestType` Enum**

Create a public enum to represent the outcome of the parsing.

```rust
// src/protocol.rs
#[derive(Debug, PartialEq)]
pub enum RequestType<'a> {
    Ssl,
    Startup(&'a [u8]), // The initial bytes, to be replayed
}
```

### **2.3. Implement `parse_request` function**

Create an asynchronous function `parse_request` that takes an `tokio::net::TcpStream` and returns the `RequestType`.

```rust
// src/protocol.rs
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

const SSL_REQUEST_CODE: u32 = 80877103;

// ... RequestType enum ...

pub async fn parse_request<'a>(
    stream: &mut TcpStream,
    buffer: &'a mut [u8; 8],
) -> anyhow::Result<RequestType<'a>> {
    stream.read_exact(buffer).await?;

    let length = u32::from_be_bytes(buffer[0..4].try_into()?);
    if length != 8 {
        return Ok(RequestType::Startup(buffer));
    }

    let code = u32::from_be_bytes(buffer[4..8].try_into()?);
    if code == SSL_REQUEST_CODE {
        Ok(RequestType::Ssl)
    } else {
        Ok(RequestType::Startup(buffer))
    }
}
```
*Note: The function will take a mutable buffer as an argument to avoid allocation.*

### **2.4. Update `src/lib.rs`**

Add the new module to `src/lib.rs`.

```rust
// src/lib.rs
pub mod config;
pub mod protocol;
```

## **3. Testing Strategy (TDD)**

Testing will be done using mocked streams to simulate client connections. The `tokio_test` crate can be useful here.

### **3.1. Create Test Cases**

In `src/protocol.rs`, inside a `#[cfg(test)]` module, create the following tests:

1.  **`test_parse_ssl_request`**:
    *   Create a mock stream that will provide the exact 8-byte `SSLRequest` sequence (`[0, 0, 0, 8, 4, 210, 22, 47]`).
    *   Call `parse_request` with the mock stream.
    *   Assert that the result is `Ok(RequestType::Ssl)`.

2.  **`test_parse_startup_message`**:
    *   Create a mock stream that provides a sequence of bytes representing a typical PostgreSQL `StartupMessage`. A key characteristic is that the first 4 bytes (length) will not be 8. For example, use a real captured `StartupMessage` or a plausible fake.
    *   Call `parse_request`.
    *   Assert that the result is `Ok(RequestType::Startup(..))` and that the returned slice contains the exact bytes that were sent.

3.  **`test_parse_invalid_ssl_request`**:
    *   Create a mock stream that provides a sequence of 8 bytes where the length is 8 but the code is *not* `80877103`.
    *   Call `parse_request`.
    *   Assert that the result is `Ok(RequestType::Startup(..))`, as this should be treated as a regular startup message.

4.  **`test_stream_eof`**:
    *   Create a mock stream that provides fewer than 8 bytes before closing.
    *   Call `parse_request`.
    *   Assert that the function returns an `Err` indicating an unexpected EOF.
