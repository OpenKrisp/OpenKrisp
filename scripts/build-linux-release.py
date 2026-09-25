#!/usr/bin/env python3
"""Build the full app version, then produce a distribution-compatible Linux package."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CONFIG = "src-tauri/tauri.community.linux.conf.json"


def package_versions(version):
    # The release guard validates full SemVer before this job. Native package
    # managers use their own ordering: a beta must precede the stable version.
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("Expected a release version without build metadata")
    core, _, prerelease = version.partition("-")
    return (core + ("~" + prerelease if prerelease else ""), core,
            "0." + prerelease.replace("-", "_") if prerelease else "1")


def run(*args):
    subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True)


def deb_control_version(control, expected, native):
    text = control.read_text()
    if re.findall(r"^Version: (.+)$", text, re.M) != [expected]:
        raise ValueError("Unexpected original DEB version")
    control.write_text(re.sub(r"^Version: .+$", "Version: " + native, text, flags=re.M))


def build(kind, version):
    if json.loads((ROOT / "apps/desktop/src-tauri/tauri.conf.json").read_text())["version"] != version:
        raise ValueError("Build version must match the validated source")
    deb_version, rpm_version, rpm_release = package_versions(version)
    tauri = ["npm", "run", "--prefix", "apps/desktop", "tauri", "--"]
    run(*tauri, "build", "--ci", "--no-bundle", "--config", CONFIG, "--", "--locked", "--offline")
    # bundle only: this does not recompile the app with the RPM metadata version.
    config = json.loads((ROOT / "apps/desktop" / CONFIG).read_text())
    if kind == "rpm":
        config["version"] = rpm_version
        config["bundle"]["linux"]["rpm"]["release"] = rpm_release
    run(*tauri, "bundle", "--ci", "--bundles", kind, "--config", json.dumps(config))
    packages = list((ROOT / "target/release/bundle" / kind).glob(f"*.{kind}"))
    if len(packages) != 1:
        raise ValueError("Expected exactly one newly built Linux package")
    if kind == "deb" and deb_version != version:
        package = packages[0]
        with tempfile.TemporaryDirectory(prefix="openkrisp-deb-") as temporary:
            payload = Path(temporary) / "payload"
            run("dpkg-deb", "--raw-extract", package, payload)
            deb_control_version(payload / "DEBIAN/control", version, deb_version)
            run("dpkg-deb", "--build", "--root-owner-group", payload, package)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=["deb", "rpm"], required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    build(args.format, args.version)
