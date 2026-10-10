#!/usr/bin/env bash
set -euo pipefail

# Keep this repository's package caches local, away from personal caches and credentials.
# Separate root container jobs, native jobs and architectures on the same runner.
task_cache="${RUNNER_TOOL_CACHE:?}/openkrisp-${RUNNER_OS:?}-${RUNNER_ARCH:?}-$(id -u)"
mkdir -p "$task_cache/cargo" "$task_cache/npm"
{
  printf 'CARGO_HOME=%s\n' "$task_cache/cargo"
  printf 'npm_config_cache=%s\n' "$task_cache/npm"
  printf 'NPM_CONFIG_USERCONFIG=/dev/null\n'
} >> "${GITHUB_ENV:?}"

if [[ "${1:-}" == --target ]]; then
  if [[ -e target || -L target ]]; then
    echo 'Checkout must be clean before attaching the local build cache.' >&2
    exit 1
  fi
  mkdir -p "$task_cache/target"
  # Keep compiler artifacts, but regenerate reports and packages for this job.
  # The installer builder deliberately refuses to overwrite an existing package.
  for generated in test-evidence debug/bundle release/bundle native-smoke; do
    rm -rf -- "$task_cache/target/$generated"
  done
  ln -s "$task_cache/target" target
  # Tauri's updater rejects executables launched through a symlink on macOS.
  # Cargo launches tests from the real path; existing scripts can still read target/.
  printf 'CARGO_TARGET_DIR=%s\n' "$(cd "$task_cache/target" && pwd -P)" >> "$GITHUB_ENV"
fi
