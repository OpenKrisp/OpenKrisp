#!/usr/bin/env bash
set -euo pipefail
if [[ ! -f /.dockerenv ]]; then
  echo 'Run this script only in a disposable Docker container.' >&2
  exit 1
fi
# Run in a disposable Debian Rust container, with the repository mounted read-only at /workspace.
apt-get update
apt-get install --yes --no-install-recommends g++ python3-venv pkg-config ca-certificates git
mkdir -p /work
tar -C /workspace --exclude=./target --exclude=./.git --exclude=./apps/desktop/node_modules --exclude=./apps/desktop/dist -cf - . | tar -C /work -xf -
cd /work
python3 -m venv target/aec-tools
target/aec-tools/bin/pip install meson==1.9.2 ninja==1.13.0
cargo test --locked -p aec-webrtc -- --nocapture
python3 scripts/probe-aec-native.py
python3 scripts/probe-aec-suppression.py
python3 scripts/probe-aec-suppression.py debug --path-changes
