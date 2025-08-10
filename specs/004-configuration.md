# **Specification 004: Configuration**

## **1. Introduction**

This document specifies the configuration format for `pgtls`. A clear and comprehensive configuration scheme is essential for making the proxy flexible and easy to use. The configuration model supports defining multiple, independent proxy instances, each mapping a single listener to a single backend.

## **2. Configuration Format**

The proxy shall be configured via a TOML file. TOML is chosen for its clear syntax and good support in the Rust ecosystem (via the `serde` and `toml` crates).

The path to the configuration file will be provided via a command-line argument.

## **3. Configuration Schema**

The TOML configuration file will be structured into a global settings section and an array of `[[proxy]]` tables.

### **3.1. Global Settings**

This section contains settings that apply to the proxy as a whole.

- `log_level`: (Optional) The logging level. Can be one of `trace`, `debug`, `info`, `warn`, `error`. Defaults to `info`.

### **3.2. `[[proxy]]` - Proxy Route Definition**

This is an array of tables, where each element defines a self-contained proxy route from a specific listening address to a specific backend.

Each `[[proxy]]` table has two sub-tables: `listener` and `backend`.

#### **3.2.1. `[proxy.listener]` - Client-Facing Listener**

- `bind_address`: (Required) The address and port on which the proxy will listen for client connections. Example: `"0.0.0.0:6432"`.
- `server_cert`: (Required) The file path to the server certificate that the proxy will present to clients.
- `server_key`: (Required) The file path to the private key for the server certificate.
- `mtls`: (Optional) A boolean value (`true` or `false`) to enable or disable client certificate verification (mTLS) for this listener. Defaults to `false`.
- `client_ca`: (Optional) The file path to the client CA certificate bundle used to verify client certificates. Required if `mtls` is `true`.

#### **3.2.2. `[proxy.backend]` - Backend Server**

- `address`: (Required) The address (hostname or IP) and port of the backend PostgreSQL server. Example: `"127.0.0.1:5432"`.
- `tls_enabled`: (Optional) A boolean (`true` or `false`) to control whether to use TLS for the connection to the backend server. Defaults to `true`.
- `root_ca`: (Optional) The file path to the root CA certificate bundle used to verify the backend server's certificate. Required if `tls_enabled` is `true`.
- `client_cert`: (Optional) The file path to the client certificate that the proxy will present to the backend server for mTLS. Only used if `tls_enabled` is `true`.
- `client_key`: (Optional) The file path to the private key for the proxy's client certificate. Required if `client_cert` is set. Only used if `tls_enabled` is `true`.

## **4. Example Configuration File**

```toml
# Global settings
log_level = "info"

# Proxy route #1: Public-facing listener to a secure, TLS-enabled backend.
# Requires clients to present a valid certificate (mTLS).
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

# Proxy route #2: Internal listener that adds TLS to a legacy backend
# that does not have TLS configured.
[[proxy]]
  [proxy.listener]
  bind_address = "127.0.0.1:6433"
  server_cert = "/etc/pgtls/certs/internal-proxy.pem"
  server_key = "/etc/pgtls/certs/internal-proxy.key"
  mtls = false

  [proxy.backend]
  address = "10.0.1.50:5432"
  tls_enabled = false
```

## **5. Configuration Loading and Validation**

- The proxy will parse the TOML file at startup.
- The implementation must perform validation on each `[[proxy]]` entry:
  - All required fields must be present.
  - All specified file paths must exist and be readable.
  - If `backend.tls_enabled` is `true`, `backend.root_ca` must be present.
  - `backend.client_key` must be present if `backend.client_cert` is.
  - `listener.client_ca` must be present if `listener.mtls` is `true`.
- Clear and actionable error messages should be provided for any configuration errors.
