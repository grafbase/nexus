#!/bin/bash

# Create directory for certificates
mkdir -p docker/redis/tls

# Generate CA key and certificate
openssl genrsa -out docker/redis/tls/ca.key 4096
openssl req -new -x509 -days 3650 -key docker/redis/tls/ca.key -out docker/redis/tls/ca.crt \
    -subj "/C=US/ST=CA/L=San Francisco/O=TestCA/CN=TestCA"

# Generate server key and certificate
openssl genrsa -out docker/redis/tls/server.key 4096
openssl req -new -key docker/redis/tls/server.key -out docker/redis/tls/server.csr \
    -subj "/C=US/ST=CA/L=San Francisco/O=Test/CN=localhost"
openssl x509 -req -days 3650 -in docker/redis/tls/server.csr -CA docker/redis/tls/ca.crt \
    -CAkey docker/redis/tls/ca.key -CAcreateserial -out docker/redis/tls/server.crt

# Generate client key and certificate
openssl genrsa -out docker/redis/tls/client.key 4096
openssl req -new -key docker/redis/tls/client.key -out docker/redis/tls/client.csr \
    -subj "/C=US/ST=CA/L=San Francisco/O=Test/CN=client"
openssl x509 -req -days 3650 -in docker/redis/tls/client.csr -CA docker/redis/tls/ca.crt \
    -CAkey docker/redis/tls/ca.key -CAcreateserial -out docker/redis/tls/client.crt

# Clean up CSR files
rm -f docker/redis/tls/*.csr

# Set permissions
chmod 644 docker/redis/tls/*.crt
chmod 600 docker/redis/tls/*.key

echo "TLS certificates generated successfully in docker/redis/tls/"