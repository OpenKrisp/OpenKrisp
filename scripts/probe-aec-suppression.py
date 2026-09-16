"""Build/run the offline native linear-output versus residual-suppression diagnostic.

Run after cargo test -p aec-webrtc. Experimental profiles exist only in this executable;
no audio devices, microphone recordings, application tuning or PCM IPC are involved.
"""
from pathlib import Path
import os
import platform
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
profile = sys.argv[1] if len(sys.argv) > 1 else "debug"
probe_args = sys.argv[2:]
if profile not in {"debug", "release"} or probe_args not in ([], ["--path-changes"], ["--alignment"]):
    raise SystemExit("Usage: probe-aec-suppression.py [debug|release] [--path-changes|--alignment]")
archives = list((root / "target" / profile / "build").glob("aec-webrtc-*/out/libopenkrisp_aec_native.a"))
if not archives:
    raise SystemExit("Build aec-webrtc first")
archive = max(archives, key=lambda p: p.stat().st_mtime_ns)
native = archive.parent / "sources/webrtc-audio-processing-sys-2.1.0/webrtc-audio-processing"
if (native / "openkrisp_bridge.cc").read_bytes() != (root / "crates/aec-webrtc/src/bridge.cc").read_bytes():
    raise SystemExit("Native build is stale; rebuild aec-webrtc first")
for source, built in [
    ("config.h", "openkrisp_aec_config.h"),
    ("render_alignment.h", "webrtc/modules/audio_processing/aec3/openkrisp_render_alignment.h"),
]:
    if (native / built).read_bytes() != (root / "crates/aec-webrtc/src" / source).read_bytes():
        raise SystemExit("Native profile is stale; rebuild aec-webrtc first")
output = archive.parent / "suppression-probe"
flags = ["-DWEBRTC_POSIX", "-DWEBRTC_APM_DEBUG_DUMP=0"]
libraries = ["-pthread"]
if platform.system() == "Darwin":
    flags += ["-DWEBRTC_MAC"]
    libraries = ["-framework", "Foundation"]
else:
    flags += ["-DWEBRTC_LINUX"]
includes = ["-I" + str(p) for p in (native, native / "webrtc", native / "subprojects/abseil-cpp-20240722.0")]
subprocess.run([os.environ.get("CXX", "c++"), "-std=c++17", "-O2", "-DNDEBUG", *flags, *includes,
                str(root / "crates/aec-webrtc/tests/suppression_probe.cc"), str(archive),
                *libraries, "-o", str(output)], check=True)
subprocess.run([str(output), *probe_args], check=True)
