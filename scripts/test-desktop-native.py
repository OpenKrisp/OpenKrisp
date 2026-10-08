#!/usr/bin/env python3
"""Run a packaged --features desktop-smoke app with private settings and bounded timeouts."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, default=Path("target/test-evidence/native"))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    for mode in ("fixture", "startup"):
        with tempfile.TemporaryDirectory(prefix="openkrisp-native-") as temporary:
            result = subprocess.run([str(args.binary.resolve()), f"--smoke-{mode}", temporary], capture_output=True, text=True, timeout=60)
            (args.output / f"{mode}.log").write_text(result.stdout + result.stderr)
            source = Path(temporary) / "report.json"
            if not source.is_file():
                raise RuntimeError(f"native {mode} produced no report (exit {result.returncode}): {result.stderr}")
            report = json.loads(source.read_text())
            required = {"packaged_model_integrity", "native_webview_ipc", "frontend_renders_native_state", "close_hides_window", "tray_show_restores_window", "reload_reconciles_native_state", "quit_flushes_settings_and_finishes_shutdown"}
            if mode == "fixture":
                required.add("hidden_routes_preserved")
            if result.returncode or not report.get("passed") or not required <= set(report["evidence"]):
                raise RuntimeError(f"native {mode} failed: {report}; {result.stderr}")
            (args.output / f"{mode}.json").write_text(json.dumps(report, indent=2) + "\n")
            print(f"native {mode}: passed ({len(report['evidence'])} checks)")

if __name__ == "__main__":
    main()
