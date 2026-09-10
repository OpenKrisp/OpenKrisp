"""Offline native AEC build. Requires Python 3, C/C++, Meson 1.9.2 and Ninja 1.13.0.

Install tools in target/aec-tools, set OPENKRISP_AEC_TOOLS to their bin directory,
or provide them on PATH. Source and build definitions are hash-pinned in the repository.
No downloads, system Abseil lookup, or system installation occurs here.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "third_party/aec"


def extract(archive, destination):
    with tarfile.open(archive) as source:
        for member in source.getmembers():
            target = (destination / member.name).resolve()
            if not target.is_relative_to(destination.resolve()) or not (member.isfile() or member.isdir()):
                raise ValueError(f"Unsafe archive member: {member.name}")
        source.extractall(destination)


def verify_sources(directory):
    for name, expected in json.loads((directory / "SHA256.json").read_text()).items():
        if hashlib.sha256((directory / name).read_bytes()).hexdigest() != expected:
            raise ValueError(f"AEC source integrity failure: {name}")


def main():
    out = Path(sys.argv[1]).resolve()
    jobs = min(8, max(1, int(sys.argv[2])))
    verify_sources(VENDOR)
    env = os.environ.copy()
    tools = Path(env.get("OPENKRISP_AEC_TOOLS", ROOT / "target/aec-tools/bin"))
    env["PATH"] = str(tools) + os.pathsep + env["PATH"]
    for name, expected in [("meson", "1.9.2"), ("ninja", "1.13.0")]:
        actual = subprocess.check_output([name, "--version"], env=env, text=True).strip()
        if actual not in {expected, expected + ".git.kitware.jobserver-pipe-1"}:
            raise ValueError(f"Expected {name} {expected}, found {actual}")
    sources = out / "sources"
    sources.mkdir(parents=True, exist_ok=True)
    extract(VENDOR / "webrtc-audio-processing-sys-2.1.0.crate", sources)
    native = sources / "webrtc-audio-processing-sys-2.1.0/webrtc-audio-processing"
    abseil_root = native / "subprojects"
    extract(VENDOR / "abseil-cpp-20240722.0.tar.gz", abseil_root)
    abseil = abseil_root / "abseil-cpp-20240722.0"
    shutil.copyfile(VENDOR / "abseil-meson.build", abseil / "meson.build")
    config = native / "meson.build"
    text = config.read_text()
    start = text.index("absl_dep = [")
    end = text.index("\n]\n", start) + 3
    names = ["base", "flags", "strings", "numeric", "synchronization", "types"]
    replacement = "absl_pinned = subproject('abseil-cpp', default_options: ['cpp_std=c++17'])\nabsl_dep = [\n"
    replacement += "\n".join(f"  absl_pinned.get_variable('absl_{name}_dep')," for name in names)
    replacement += "\n]\n"
    text = text[:start] + replacement + text[end:]
    # No system dependency resolution or wrap network fallback. All native outputs are static.
    text += "\nstatic_library('openkrisp_aec_bridge', 'openkrisp_bridge.cc', dependencies: common_deps, include_directories: webrtc_inc, cpp_args: common_cxxflags + ['-DWEBRTC_APM_DEBUG_DUMP=0'])\n"
    config.write_text(text)
    shutil.copyfile(ROOT / "crates/aec-webrtc/src/bridge.cc", native / "openkrisp_bridge.cc")
    build = out / "native-build"
    args = ["meson", "setup", str(build), str(native), "--wrap-mode=nodownload",
            "-Ddefault_library=static", "-Dbuildtype=release", "-Db_ndebug=true"]
    if (build / "build.ninja").exists():
        args.append("--reconfigure")
    subprocess.run(args, env=env, check=True)
    subprocess.run(["ninja", "-C", str(build), "-j", str(jobs)], env=env, check=True)
    archives = sorted(build.rglob("*.a"))
    if not archives:
        raise ValueError("No native archives produced")
    combined = out / "libopenkrisp_aec_native.a"
    if platform.system() == "Darwin":
        subprocess.run(["/usr/bin/libtool", "-static", "-o", str(combined), *map(str, archives)], check=True)
    else:
        # MRI paths are quoted; these are our build paths, never shell command text.
        lines = [f'CREATE "{combined}"'] + [f'ADDLIB "{p}"' for p in archives] + ["SAVE", "END", ""]
        subprocess.run(["ar", "-M"], input="\n".join(lines), text=True, check=True)


if __name__ == "__main__":
    main()
