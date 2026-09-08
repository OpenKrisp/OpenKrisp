#!/usr/bin/env bash
set -euo pipefail
# Container-only dependency installation test; never run this on a user's desktop.
# docker run --rm -v "$PWD:/workspace:ro" -w /workspace rust:1.98-slim-bookworm \
#   bash scripts/test-debian-audio-dependencies.sh
if [[ ! -f /.dockerenv ]]; then
  echo 'Run this test in a disposable Docker container.' >&2
  exit 1
fi
apt-get update
apt-get install --yes --no-install-recommends python3
python3 - <<'PY'
import json
from pathlib import Path
import subprocess
import tempfile

config = json.loads(Path('apps/desktop/src-tauri/tauri.conf.json').read_text())
dependencies = config['bundle']['linux']['deb']['depends']
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory) / 'root'
    (root / 'DEBIAN').mkdir(parents=True)
    (root / 'DEBIAN/control').write_text(
        'Package: openkrisp-audio-dependency-test\nVersion: 0.1.0\n'
        'Architecture: all\nMaintainer: OpenKrisp test fixture <noreply@example.invalid>\n'
        'Description: Test OpenKrisp runtime dependency resolution\n'
        f'Depends: {", ".join(dependencies)}\n'
    )
    package = Path(directory) / 'dependencies.deb'
    subprocess.run(['dpkg-deb', '--build', str(root), str(package)], check=True)
    subprocess.run(['apt-get', 'install', '--yes', '--no-install-recommends', str(package)], check=True)
PY
pipewire --version
pw-loopback --version
wireplumber --version
pipewire-pulse --version
echo 'Debian resolved and installed the complete declared audio dependencies.'
