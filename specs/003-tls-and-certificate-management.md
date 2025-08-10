# **Specification 003: TLS and Certificate Management**

## **1. Introduction**

This document specifies the requirements for TLS session handling and certificate management within `pgtls`. For each `[[proxy]]` route defined in the configuration, the application will create a distinct set of TLS configurations.

## **2. TLS Library**

The proxy shall use `rustls` as its TLS library, integrated with the `tokio` asynchronous runtime via the `tokio-rustls` crate. This choice is motivated by `rustls`'s focus on memory safety and modern cryptographic practices.

## **3. Client-Facing TLS Configuration**

This configuration corresponds to the `[proxy.listener]` section in the configuration file and defines how the proxy presents itself to connecting clients for a specific route.

### **3.1. Server Certificate and Private Key**

*   The proxy must be configured with a path to a server certificate (in PEM format) and a corresponding private key.
*   The certificate's Common Name (CN) or Subject Alternative Name (SAN) should match the hostname that clients will use to connect to the proxy, to support `verify-full` mode.
*   The implementation will use `rustls::ServerConfig` to build the server-side TLS context.

### **3.2. Client Certificate Authentication (mTLS)**

*   The proxy shall optionally support Mutual TLS (mTLS), where it verifies the client's identity.
*   When mTLS is enabled for a listener, the proxy must be configured with a path to a client Certificate Authority (CA) bundle.
*   The proxy will use this CA to verify the certificates presented by connecting clients.
*   The `rustls::server::WebPkiClientVerifier` will be used to build the client certificate verifier.

## **4. Backend Connection Configuration**

This configuration corresponds to the `[proxy.backend]` section in the configuration file and defines how the proxy connects to the backend PostgreSQL server for a specific route.

### **4.1. Plaintext Connection**

The proxy establishes direct TCP connections to backend PostgreSQL servers without TLS encryption. All client TLS connections are terminated at the proxy, and data is forwarded to the backend as plaintext.

*Note: This simplified architecture is designed for environments where the network between the proxy and backend database is trusted (e.g., within the same secure network segment or container cluster).*

## **5. Certificate and Key Loading**

*   During startup, for each `[[proxy]]` route, the application shall load the specified certificates and private keys from the filesystem.
*   The implementation must handle potential I/O errors and parsing errors during the loading process and provide clear error messages that include which proxy route failed.
*   For each route, the resulting `rustls::ServerConfig` should be created and passed to the listener task for that route. This configuration should be wrapped in an `Arc` to be shared efficiently among all connection handlers for that specific listener.

## **6. Summary of Required Credentials**

The following table summarizes the credentials that need to be managed.

| Credential                               | Purpose                                                     | `rustls` Configuration | Required?                               |
| :--------------------------------------- | :---------------------------------------------------------- | :--------------------- | :-------------------------------------- |
| **Proxy's Server Certificate & Key**     | Presented to clients connecting to the proxy.               | `ServerConfig`         | Yes                                     |
| **Client CA Bundle**                     | To verify certificates presented by clients (for mTLS).     | `ServerConfig`         | No (Optional, based on mTLS setting)   |
