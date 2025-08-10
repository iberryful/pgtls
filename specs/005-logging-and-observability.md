# **Specification 005: Logging and Observability**

## **1. Introduction**

For a network proxy, robust logging and observability are not optional features; they are essential for debugging, monitoring, and security analysis. This document specifies the logging strategy for `pgtls`.

## **2. Logging Framework**

*   The proxy shall use the `tracing` crate as its logging and diagnostics framework. `tracing` is the de facto standard in the asynchronous Rust ecosystem.
*   The `tracing-subscriber` crate will be used to configure how logs are processed and output.

## **3. Log Levels**

The application will support standard log levels, configurable via the `log_level` setting in the configuration file.

*   **`ERROR`**: For critical errors that prevent the proxy from functioning correctly (e.g., failed to bind to a port, configuration parsing errors).
*   **`WARN`**: For non-critical issues that may indicate a problem (e.g., a client disconnected abruptly, received a non-SSL request on a TLS-only listener).
*   **`INFO`**: For high-level operational messages (e.g., service startup, a new client connection was accepted, successful shutdown).
*   **`DEBUG`**: For detailed diagnostic information useful for debugging (e.g., state transitions in the connection handler, certificate details).
*   **`TRACE`**: For highly verbose, low-level information (e.g., details of I/O operations). Not intended for production use.

The default log level shall be `INFO`.

## **4. Structured Logging**

*   Logs should be structured to be machine-parsable, preferably in JSON format. This allows for easy integration with log aggregation and analysis tools (e.g., ELK stack, Splunk, Datadog).
*   The `tracing_subscriber::fmt::json()` formatter can be used to achieve this.

### **4.1. Log Fields**

Each log entry should contain a consistent set of fields to provide context.

*   `timestamp`: The time the event occurred.
*   `level`: The log level (`INFO`, `WARN`, etc.).
*   `message`: The main log message.
*   `target`: The module path where the log originated (e.g., `pgtls::connection_handler`).

### **4.2. Contextual Fields**

Crucially, logs related to a specific connection should be enriched with contextual information. `tracing`'s `span`s are the ideal mechanism for this. A span should be created for each connection and should include fields like:

*   `client_addr`: The IP address and port of the connecting client.
*   `listener_addr`: The address of the listener that accepted the connection.
*   `connection_id`: A unique identifier for the connection (e.g., a simple counter or a UUID) to correlate all log entries for a single session.

An example of a log entry within a connection span:

```json
{
  "timestamp": "2025-08-10T02:31:51.887Z",
  "level": "INFO",
  "message": "TLS handshake with client completed.",
  "target": "pgtls::connection_handler",
  "fields": {
    "client_addr": "192.168.1.100:54321",
    "listener_addr": "0.0.0.0:6432",
    "connection_id": "conn-123"
  }
}
```

## **5. Key Logging Events**

The implementation must ensure that logs are generated for the following critical events:

*   Service startup and shutdown.
*   Configuration loading (success or failure).
*   Listener successfully binding to a port.
*   Acceptance of a new client connection.
*   Detection of `SSLRequest` vs. `StartupMessage`.
*   Success or failure of client-facing TLS handshake.
*   Success or failure of server-facing TLS handshake.
*   Connection termination (both graceful and abrupt), including the reason if possible.
*   Any I/O errors during data streaming.
*   All certificate loading and validation errors.
