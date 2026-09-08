#!/usr/bin/env bash
set -euo pipefail
# Run under dbus-run-session on a Linux CI/container with pipewire-bin and wireplumber.
# Use a private runtime directory: this test never connects to the user's audio graph.
cargo clippy --locked -p audio-cpal -p audio-engine -p platform-routing --all-targets -- -D warnings
test_runtime=$(mktemp -d)
export XDG_RUNTIME_DIR="$test_runtime"
export PIPEWIRE_RUNTIME_DIR="$test_runtime"
export OPENKRISP_PIPEWIRE_TEST=1
pipewire >"$test_runtime/pipewire.log" 2>&1 &
pipewire_pid=$!
wireplumber_pid=
fixture_pid=
cleanup() {
  if [[ -n "$fixture_pid" ]]; then kill "$fixture_pid" 2>/dev/null || true; wait "$fixture_pid" 2>/dev/null || true; fi
  if [[ -n "$wireplumber_pid" ]]; then kill "$wireplumber_pid" 2>/dev/null || true; wait "$wireplumber_pid" 2>/dev/null || true; fi
  kill "$pipewire_pid" 2>/dev/null || true
  wait "$pipewire_pid" 2>/dev/null || true
  if [[ "${test_status:-1}" != 0 ]]; then cat "$test_runtime/pipewire.log" "$test_runtime/wireplumber.log" "$test_runtime/fixture.log" 2>/dev/null || true; fi
  rm -rf -- "$test_runtime"
}
trap cleanup EXIT
for _ in {1..100}; do
  [[ -S "$test_runtime/pipewire-0" ]] && break
  sleep 0.05
done
test -S "$test_runtime/pipewire-0"
# WirePlumber 0.4's Bluetooth/logind component cannot initialize in a container without
# systemd seats. Disable that hardware monitor in a private copy of the test configuration.
if wireplumber --version | grep -q '0\.4\.'; then
  cp -R /usr/share/wireplumber "$test_runtime/wireplumber"
  sed '/name = bluetooth.lua/d' /usr/share/wireplumber/wireplumber.conf > "$test_runtime/wireplumber/wireplumber.conf"
  export WIREPLUMBER_CONFIG_DIR="$test_runtime/wireplumber"
fi
wireplumber >"$test_runtime/wireplumber.log" 2>&1 &
wireplumber_pid=$!
# CPAL intentionally rejects a server with no audio nodes. This private, silent fixture
# substitutes for hardware in CI; its names are outside the supported OpenKrisp endpoint pair.
pw-loopback --name=openkrisp_test --channels=2 \
  --capture-props='media.class=Audio/Sink node.name=openkrisp_test_sink node.virtual=true node.autoconnect=false' \
  --playback-props='media.class=Audio/Source node.name=openkrisp_test_source node.virtual=true node.autoconnect=false' \
  >"$test_runtime/fixture.log" 2>&1 &
fixture_pid=$!
for _ in {1..100}; do
  if pw-cli ls Node | grep -q 'openkrisp_test_source'; then break; fi
  sleep 0.05
done
pw-cli ls Node | grep -q 'openkrisp_test_source'
cargo test --locked -p platform-routing --test pipewire_live -- --ignored --nocapture
test_status=0
