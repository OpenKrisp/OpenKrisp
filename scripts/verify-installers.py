#!/usr/bin/env python3
"""Inspect generated installer artifacts without installing them."""

import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


def output(*args):
    return subprocess.check_output([str(arg) for arg in args], text=True)


def verify_macos(package):
    pins = json.loads((ROOT / "packaging/macos/blackhole.json").read_text())
    with tempfile.TemporaryDirectory(prefix="openkrisp-verify-") as temporary:
        expanded = Path(temporary) / "expanded"
        subprocess.run(["pkgutil", "--expand-full", str(package), str(expanded)], check=True)
        distribution = ET.parse(expanded / "Distribution").getroot()
        driver_arguments = []
        for driver in pins["drivers"]:
            identifier = driver["identifier"]
            choice = distribution.find(f"choice[@id='{identifier}']")
            assert choice is not None and choice.get("selected") == "needsDriver('%s', '%s')" % (driver["bundle"], identifier)
            reference = distribution.find(f"pkg-ref[@id='{identifier}']")
            assert reference is not None and reference.get("onConclusion") == "RequireRestart"
            component = expanded / (driver["bundle"] + ".pkg")
            driver_metadata = ET.parse(component / "PackageInfo").getroot()
            assert driver_metadata.get("identifier") == identifier
            assert driver_metadata.get("install-location") == "/Library/Audio/Plug-Ins/HAL"
            assert driver_metadata.get("version") == pins["driver_version"]
            assert not (component / "Scripts").exists(), "Drivers must not mutate shared installations"
            bundle = component / "Payload" / (driver["bundle"] + ".driver")
            with (bundle / "Contents/Info.plist").open("rb") as stream:
                info = plistlib.load(stream)
            assert info["CFBundleIdentifier"] == identifier
            assert info["CFBundleShortVersionString"] == pins["driver_version"]
            assert info["CFPlugInFactories"] == {driver["factory_uuid"]: "BlackHole_Create"}
            assert info["CFBundleName"] == driver["name"]
            binary = bundle / "Contents/MacOS" / info["CFBundleExecutable"]
            assert set(output("lipo", "-archs", binary).split()) == {"arm64", "x86_64"}
            subprocess.run(["codesign", "--verify", "--strict", str(bundle)], check=True)
            driver_arguments.extend([str(bundle), driver["name"], driver["uid"], str(driver["channels"])])
        assert len(distribution.findall("choice")) == 3
        assert not list(expanded.glob("BlackHole*.pkg"))
        probe = Path(temporary) / "probe-driver"
        subprocess.run(["xcrun", "clang", "-Wall", "-Wextra", "-Werror",
                        str(ROOT / "scripts/probe-macos-driver.c"),
                        "-framework", "CoreAudio", "-framework", "CoreFoundation", "-o", str(probe)], check=True)
        subprocess.run([str(probe), *driver_arguments], check=True)
        payload = expanded / "OpenKrisp-app.pkg/Payload"
        project_notices = payload / "Library/Application Support/OpenKrisp"
        license_resource = distribution.find("license")
        assert license_resource is not None and license_resource.get("file") == "Licenses.txt"
        displayed_terms = (expanded / "Resources/Licenses.txt").read_bytes()
        for name in ("LICENSE", "NOTICE"):
            expected = (ROOT / name).read_bytes()
            assert (project_notices / name).read_bytes() == expected
            assert expected in displayed_terms
        with (payload / "Applications/OpenKrisp.app/Contents/Info.plist").open("rb") as stream:
            app = plistlib.load(stream)
        assert app["CFBundleIdentifier"] == "app.openkrisp.desktop"
        assert (payload / "Applications/OpenKrisp.app/Contents/MacOS" / app["CFBundleExecutable"]).is_file()
        notices = payload / "Library/Application Support/OpenKrisp/ThirdParty/BlackHole"
        assert (notices / "LICENSE.txt").is_file()
        assert (notices / "LICENSE.txt").read_bytes() in displayed_terms
        assert json.loads((notices / "provenance.json").read_text()) == pins
        assert hashlib.sha256((notices / "BlackHole-source.tar.gz").read_bytes()).hexdigest() == pins["source"]["sha256"]
        for relative in ["scripts/build-macos-installer.py", "packaging/macos/blackhole.json",
                         "packaging/macos/welcome.html", "packaging/macos/conclusion.html",
                         "apps/desktop/src-tauri/icons/icon.icns"]:
            assert (notices / "build" / relative).read_bytes() == (ROOT / relative).read_bytes()
        print("macOS package: app, named native drivers, source/build inputs, signatures and restart choices verified.")


def verify_linux_payload(payload, notices):
    executable = payload / "usr/bin/openkrisp-desktop"
    header = executable.read_bytes()[:20]
    assert header[:6] == b"\x7fELF\x02\x01" and header[18:20] == b"\x3e\x00", "Expected x86-64 ELF executable"
    resources = payload / "usr/lib/OpenKrisp"
    for name in ["LICENSE", "NOTICE", "models/LICENSE", "models/LICENSE-MIT", "models/LICENSE-APACHE",
                 "models/manifest.json", "models/DeepFilterNet3_ll_onnx.tar.gz"]:
        assert (resources / name).read_bytes() == (ROOT / name).read_bytes(), f"Mismatched resource: {name}"
    for source, installed in [(ROOT / "third_party/aec/notices", resources / "third-party/aec/notices"),
                              (notices, resources / "third-party/dependencies")]:
        expected = {p.relative_to(source) for p in source.rglob("*") if p.is_file()}
        actual = {p.relative_to(installed) for p in installed.rglob("*") if p.is_file()}
        assert expected and actual == expected, "Missing or unexpected bundled dependency notices"
        for relative in expected:
            assert (source / relative).read_bytes() == (installed / relative).read_bytes(), relative


def verify_linux(package, release_version=None, notices=None):
    if package.suffix == ".deb":
        dependencies = output("dpkg-deb", "--field", package, "Depends")
        expected = ["libpipewire-0.3-0 (>= 0.3.53)", "pipewire (>= 0.3.53)",
                    "pipewire-bin", "wireplumber", "pipewire-pulse"]
        actual = {item.strip() for item in dependencies.split(",")}
    else:
        dependencies = output("rpm", "-qp", "--requires", package)
        expected = ["(pipewire-libs >= 0.3.53 and pipewire >= 0.3.53)",
                    "pipewire-utils", "wireplumber", "pipewire-pulseaudio"]
        actual = set(dependencies.splitlines())
    missing = set(expected) - actual
    assert not missing, f"{package}: missing runtime dependencies: {missing}"
    if release_version is not None:
        config = json.loads((ROOT / "apps/desktop/src-tauri/tauri.community.linux.conf.json").read_text())
        kind = package.suffix[1:]
        required = set(config["bundle"]["linux"][kind]["depends"])
        assert not required - actual, f"Missing community GUI/audio dependencies: {required - actual}"
        if kind == "deb":
            assert output("dpkg-deb", "--field", package, "Architecture").strip() == "amd64"
            assert output("dpkg-deb", "--field", package, "Version").strip() == release_version.replace("-", "~", 1)
        else:
            core, _, prerelease = release_version.partition("-")
            expected_release = "0." + prerelease.replace("-", "_") if prerelease else "1"
            assert output("rpm", "-qp", "--queryformat", "%{ARCH}", package).strip() == "x86_64"
            assert output("rpm", "-qp", "--queryformat", "%{VERSION}", package).strip() == core
            assert output("rpm", "-qp", "--queryformat", "%{RELEASE}", package).strip() == expected_release
        assert notices is not None and (notices / "inventory.json").is_file(), "Release inventory is required"
        with tempfile.TemporaryDirectory(prefix="openkrisp-linux-inspect-") as temporary:
            payload = Path(temporary)
            if kind == "deb":
                subprocess.run(["dpkg-deb", "--extract", str(package.resolve()), str(payload)], check=True)
            else:
                archive = subprocess.check_output(["rpm2cpio", str(package.resolve())])
                subprocess.run(["cpio", "--extract", "--make-directories", "--no-absolute-filenames"],
                               input=archive, cwd=payload, check=True, capture_output=True)
            verify_linux_payload(payload, notices)
        print(f"{package.name}: release version, x86-64 payload, model and dependency notices verified.")
    print(f"{package.name}: full PipeWire runtime dependencies verified.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("packages", type=Path, nargs="+")
    parser.add_argument("--release-version", help="Also check Linux release metadata and payload resources")
    parser.add_argument("--notices", type=Path, default=ROOT / "target/release-notices")
    args = parser.parse_args()
    for package in args.packages:
        if package.suffix == ".pkg":
            verify_macos(package)
        elif package.suffix in (".deb", ".rpm"):
            verify_linux(package, args.release_version, args.notices)
        else:
            parser.error(f"Unsupported installer format: {package}")


if __name__ == "__main__":
    main()
