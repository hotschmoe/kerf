#!/usr/bin/env bash
# LAN dev server with a self-signed cert: WebGPU needs a secure context, so plain http://<lan-ip> falls back to
# "KERF NEEDS WEBGPU". Accept the browser's certificate warning once per device, then open https://<box-ip>:9091
set -euo pipefail
cd "$(dirname "$0")/.."
TLS=gitignored/tls
if [ ! -f "$TLS/cert.pem" ]; then
  mkdir -p "$TLS"
  openssl req -x509 -newkey rsa:2048 -nodes -days 365 -keyout "$TLS/key.pem" -out "$TLS/cert.pem" -subj "/CN=kerf-egui"
fi
exec trunk serve --tls-key-path "$TLS/key.pem" --tls-cert-path "$TLS/cert.pem" --address 0.0.0.0 --port 9091 "$@"
