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

## **4. Server-Facing Connection Configuration**

This configuration corresponds to the `[proxy.backend]` section in the configuration file and defines how the proxy connects to the backend PostgreSQL server for a specific route. The connection can be either a direct plaintext TCP connection or a TLS-encrypted connection, based on the `backend.tls_enabled` setting.

### **4.1. Plaintext Connection**

*   If `backend.tls_enabled` is `false`, the proxy will establish a direct TCP connection to the backend server and will not attempt any TLS negotiation.

### **4.2. TLS-Encrypted Connection**

If `backend.tls_enabled` is `true`, the following TLS configuration applies:

#### **4.2.1. Root Certificate Authority**

*   To securely connect to the backend, the proxy must verify the backend server's certificate.
*   The proxy must be configured with a path to a root CA bundle that can validate the backend server's certificate. This is essential for preventing man-in-the-middle attacks between the proxy and the database.
*   A `rustls::RootCertStore` will be populated with these root certificates.

#### **4.2.2. Proxy's Client Certificate (mTLS)**

*   If the backend PostgreSQL server is configured to require client certificate authentication (`cert` method in `pg_hba.conf`), the proxy must present its own client certificate.
*   The proxy shall support being configured with a path to a client certificate and a corresponding private key for this purpose.
*   This will be configured in `rustls::ClientConfig` using the `with_client_auth_cert` method.

## **5. Certificate and Key Loading**

*   During startup, for each `[[proxy]]` route, the application shall load the specified certificates and private keys from the filesystem.
*   The implementation must handle potential I/O errors and parsing errors during the loading process and provide clear error messages that include which proxy route failed.
*   For each route, the resulting `rustls` configuration objects (`ServerConfig` and `ClientConfig`) should be created and passed to the listener task for that route. These configurations should be wrapped in an `Arc` to be shared efficiently among all connection handlers for that specific listener.

## **6. Summary of Required Credentials**

The following table summarizes the credentials that need to be managed.

| Credential                               | Purpose                                                     | `rustls` Configuration | Required?                               |
| :--------------------------------------- | :---------------------------------------------------------- | :--------------------- | :-------------------------------------- |
| **Proxy's Server Certificate & Key**     | Presented to clients connecting to the proxy.               | `ServerConfig`         | Yes                                     |
| **Client CA Bundle**                     | To verify certificates presented by clients (for mTLS).     | `ServerConfig`         | No (Optional)                           |
| **Backend Root CA Bundle**               | To verify the backend PostgreSQL server's certificate.      | `ClientConfig`         | Optional (Required if backend TLS is enabled) |
| **Proxy's Client Certificate & Key**     | Presented to the backend server for mTLS authentication.    | `ClientConfig`         | Optional (Depends on backend config)    |
