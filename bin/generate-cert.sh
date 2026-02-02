#!/usr/bin/env bash
# Generate a self-signed TLS certificate for Prism development/testing.
# Usage: bin/generate-cert.sh [output-dir]
#
# Requires: openssl
set -euo pipefail

CERT_DIR="${1:-./conf/tls}"

if ! command -v openssl &>/dev/null; then
    echo "Error: openssl is required but not found in PATH" >&2
    exit 1
fi

mkdir -p "$CERT_DIR"

openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$CERT_DIR/key.pem" \
    -out "$CERT_DIR/cert.pem" \
    -days 365 \
    -subj "/CN=localhost/O=Prism Dev" \
    2>/dev/null

echo "Generated self-signed certificate in $CERT_DIR"
echo "  cert: $CERT_DIR/cert.pem"
echo "  key:  $CERT_DIR/key.pem"
echo ""
echo "To enable TLS, set in your prism.toml:"
echo ""
echo "  [server.tls]"
echo "  enabled = true"
echo "  cert_path = \"$CERT_DIR/cert.pem\""
echo "  key_path = \"$CERT_DIR/key.pem\""
