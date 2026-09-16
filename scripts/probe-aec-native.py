"""Build/run the standalone C++ new/timing probe after cargo test -p aec-webrtc.

Timing is diagnostic, not a CI performance threshold. Native source remains in Cargo OUT_DIR.
"""
from pathlib import Path
import os
import platform
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
profile = sys.argv[1] if len(sys.argv) > 1 else "debug"
archives = list((root / "target" / profile / "build").glob("aec-webrtc-*/out/libopenkrisp_aec_native.a"))
if not archives:
    raise SystemExit("Build aec-webrtc first")
archive = max(archives, key=lambda p: p.stat().st_mtime_ns)
output = archive.parent / "native-probe"
libraries = ["-framework", "Foundation"] if platform.system() == "Darwin" else ["-pthread"]
subprocess.run([os.environ.get("CXX", "c++"), "-std=c++17", "-O2", str(root / "crates/aec-webrtc/tests/native_probe.cc"), str(archive), *libraries, "-o", str(output)], check=True)
subprocess.run([str(output)], check=True)
