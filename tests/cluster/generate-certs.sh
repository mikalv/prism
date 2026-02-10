#!/usr/bin/env bash
# Generate self-signed TLS certificates for cluster integration testing.
# Creates a CA cert and per-node certs signed by that CA.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TLS_DIR="$SCRIPT_DIR/conf/tls"
mkdir -p "$TLS_DIR"

# Skip if certs already exist
if [ -f "$TLS_DIR/ca-cert.pem" ] && [ -f "$TLS_DIR/node-cert.pem" ]; then
    echo "Certificates already exist in $TLS_DIR, skipping generation."
    exit 0
fi

echo "Generating cluster TLS certificates..."

# Generate CA key and cert
openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$TLS_DIR/ca-key.pem" \
    -out "$TLS_DIR/ca-cert.pem" \
    -days 365 \
    -subj "/CN=prism-cluster-ca" \
    2>/dev/null

# Generate node key
openssl genrsa -out "$TLS_DIR/node-key.pem" 2048 2>/dev/null

# Generate node CSR with SANs for all node hostnames + localhost
cat > "$TLS_DIR/san.cnf" <<EOF
[req]
distinguished_name = req_dn
req_extensions = v3_req
prompt = no

[req_dn]
CN = prism-cluster

[v3_req]
subjectAltName = @alt_names

[alt_names]
DNS.1 = prism-node1
DNS.2 = prism-node2
DNS.3 = prism-node3
DNS.4 = localhost
IP.1 = 127.0.0.1
EOF

openssl req -new \
    -key "$TLS_DIR/node-key.pem" \
    -out "$TLS_DIR/node.csr" \
    -config "$TLS_DIR/san.cnf" \
    2>/dev/null

# Sign with CA
openssl x509 -req \
    -in "$TLS_DIR/node.csr" \
    -CA "$TLS_DIR/ca-cert.pem" \
    -CAkey "$TLS_DIR/ca-key.pem" \
    -CAcreateserial \
    -out "$TLS_DIR/node-cert.pem" \
    -days 365 \
    -extensions v3_req \
    -extfile "$TLS_DIR/san.cnf" \
    2>/dev/null

# Clean up intermediate files
rm -f "$TLS_DIR/node.csr" "$TLS_DIR/ca-cert.srl" "$TLS_DIR/san.cnf"

echo "Certificates generated in $TLS_DIR:"
echo "  CA cert:   $TLS_DIR/ca-cert.pem"
echo "  Node cert: $TLS_DIR/node-cert.pem"
echo "  Node key:  $TLS_DIR/node-key.pem"
