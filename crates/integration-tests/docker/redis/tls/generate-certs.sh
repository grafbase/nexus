#!/bin/bash
set -e

# Generate certificates for Redis TLS testing
# These are self-signed certificates for testing only

echo "Generating Redis TLS certificates..."

# Create CA key and certificate (10 years validity)
openssl genrsa -out ca.key 4096
openssl req -new -x509 -days 3650 -key ca.key -out ca.crt -subj "/C=US/ST=Test/L=Test/O=Nexus Test CA/CN=Test CA"

# Create server key and certificate signing request
openssl genrsa -out server.key 4096
openssl req -new -key server.key -out server.csr -subj "/C=US/ST=Test/L=Test/O=Nexus Test/CN=localhost"

# Create extensions file for SAN
cat > server.ext <<EOF
subjectAltName = DNS:localhost,DNS:redis-tls,IP:127.0.0.1,IP:::1
EOF

# Sign the server certificate with CA (10 years validity) including SAN
openssl x509 -req -days 3650 -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out server.crt -extfile server.ext

# Clean up extensions file
rm -f server.ext

# Create client key and certificate for mutual TLS testing
openssl genrsa -out client.key 4096
openssl req -new -key client.key -out client.csr -subj "/C=US/ST=Test/L=Test/O=Nexus Test Client/CN=test-client"

# Sign the client certificate with CA (10 years validity)
openssl x509 -req -days 3650 -in client.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out client.crt

# Clean up CSR files
rm -f server.csr client.csr ca.srl

# Set appropriate permissions
chmod 600 *.key
chmod 644 *.crt

echo "TLS certificates generated successfully!"
echo "Files created:"
echo "  - ca.crt (CA certificate)"
echo "  - ca.key (CA private key)"
echo "  - server.crt (Server certificate)"
echo "  - server.key (Server private key)"
echo "  - client.crt (Client certificate)"
echo "  - client.key (Client private key)"