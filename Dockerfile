# Multi-stage build for pgtls proxy
FROM rust:1.89-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/pgtls

# Copy Cargo files first for better layer caching
COPY Cargo.toml ./

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

# Create app user for security
RUN useradd --create-home --shell /bin/bash app

# Create app directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /usr/src/pgtls/target/release/pgtls /app/pgtls

# Change ownership to app user
RUN chown app:app /app/pgtls

# Switch to app user
USER app

# Expose the proxy ports
EXPOSE 6432

# Set the default command
CMD ["/app/pgtls", "-c", "/app/config.toml"]
