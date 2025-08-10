#!/bin/bash
set -e

echo "=== pgtls Docker E2E Test Runner ==="
echo ""

# Function to cleanup
cleanup() {
    echo "Cleaning up containers..."
    docker-compose down -v --remove-orphans 2>/dev/null || true
}

# Set trap to cleanup on exit
#trap cleanup EXIT

# Build and start infrastructure services
echo "Building pgtls Docker image..."
docker-compose build --no-cache

echo ""
echo "Starting certificate generation..."
docker-compose up cert-generator

echo ""
echo "Starting infrastructure services..."
docker-compose up -d postgres-plaintext postgres-tls

echo ""
echo "Waiting for PostgreSQL services to be healthy..."
docker-compose up --wait postgres-plaintext postgres-tls

echo ""
echo "Starting pgtls proxy service..."
docker-compose up -d pgtls-to-plaintext

echo ""
echo "Waiting for proxy services to initialize..."
sleep 10

echo ""
echo "Checking service status..."
docker-compose ps

echo ""
echo "Running end-to-end tests..."
docker-compose --profile test run --rm e2e-test

echo ""
echo "=== All tests completed successfully! ==="
