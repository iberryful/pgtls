#!/bin/sh
set -e

# Install OpenSSL
apk add --no-cache openssl

# Change to certs directory
cd /certs

# Check if certificates already exist
if [ -f "server.crt" ] && [ -f "server.key" ] && [ -f "proxy.crt" ] && [ -f "proxy.key" ]; then
    echo "Certificates already exist, skipping generation"
    ls -la /certs/
    exit 0
fi

# Generate CA private key
openssl genrsa -out ca.key 4096

# Generate CA certificate
openssl req -new -x509 -days 365 -key ca.key -out ca.crt \
    -subj "/C=US/ST=Test/L=Test/O=Test/OU=Test/CN=Test CA"

# Generate server private key
openssl genrsa -out server.key 4096

# Generate server certificate signing request
openssl req -new -key server.key -out server.csr \
    -subj "/C=US/ST=Test/L=Test/O=Test/OU=Test/CN=postgres-tls"

# Generate server certificate directly (without extensions file for simplicity)
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial \
    -out server.crt -days 365

# Generate proxy server private key
openssl genrsa -out proxy.key 4096

# Generate proxy server certificate signing request
openssl req -new -key proxy.key -out proxy.csr \
    -subj "/C=US/ST=Test/L=Test/O=Test/OU=Test/CN=pgtls-proxy"

# Generate proxy certificate
openssl x509 -req -in proxy.csr -CA ca.crt -CAkey ca.key -CAcreateserial \
    -out proxy.crt -days 365

# Generate client private key
openssl genrsa -out client.key 4096

# Generate client certificate signing request
openssl req -new -key client.key -out client.csr \
    -subj "/C=US/ST=Test/L=Test/O=Test/OU=Test/CN=test-client"

# Generate client certificate
openssl x509 -req -in client.csr -CA ca.crt -CAkey ca.key -CAcreateserial \
    -out client.crt -days 365

# Set proper permissions for PostgreSQL
chmod 600 server.key client.key proxy.key ca.key
chmod 644 server.crt client.crt proxy.crt ca.crt

# Clean up temporary files
rm -f *.csr *.srl

echo "Certificates generated successfully"
ls -la /certs/
