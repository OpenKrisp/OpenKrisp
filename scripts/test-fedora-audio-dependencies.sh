#!/usr/bin/env bash
set -euo pipefail
# Container-only dependency test. Never changes a user's desktop audio configuration.
if [[ ! -f /.dockerenv ]]; then
  echo 'Run this test in a disposable Docker container.' >&2
  exit 1
fi
dnf install --assumeyes --setopt=install_weak_deps=False python3 rpm-build
python3 - <<'PY'
import json
from pathlib import Path
import subprocess
import tempfile

config = json.loads(Path('apps/desktop/src-tauri/tauri.conf.json').read_text())
dependencies = config['bundle']['linux']['rpm']['depends']
with tempfile.TemporaryDirectory() as directory:
    spec = Path(directory) / 'dependencies.spec'
    spec.write_text(
        'Name: openkrisp-audio-dependency-test\nVersion: 0.1.0\nRelease: 1\n'
        'Summary: OpenKrisp runtime dependency fixture\nLicense: MIT\nBuildArch: noarch\n'
        + ''.join(f'Requires: {dependency}\n' for dependency in dependencies)
        + '%description\nTest dependency resolution only.\n%files\n'
    )
    subprocess.run(['rpmbuild', '-bb', '--define', f'_topdir {directory}', str(spec)], check=True)
    package, = Path(directory).glob('RPMS/noarch/*.rpm')
    subprocess.run(['dnf', 'install', '--assumeyes', '--setopt=install_weak_deps=False', str(package)], check=True)
PY
pipewire --version
pw-loopback --version
wireplumber --version
pipewire-pulse --version
echo 'Fedora resolved and installed the complete declared audio dependencies.'
