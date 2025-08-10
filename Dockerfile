# Multi-stage build for pgtls proxy
FROM rust:1.88-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/pgtls

# Copy Cargo files first for better layer caching
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src/ ./src/
COPY fixtures/ ./fixtures/

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /usr/src/pgtls/target/release/pgtls /app/pgtls

# Make the binary executable
RUN chmod +x /app/pgtls

# Expose the proxy ports
EXPOSE 6432 6433

# Set the default command
CMD ["/app/pgtls", "/app/config.toml"]
