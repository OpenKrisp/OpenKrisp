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


def verify_linux(package):
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
    print(f"{package.name}: full PipeWire runtime dependencies verified.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("packages", type=Path, nargs="+")
    for package in parser.parse_args().packages:
        if package.suffix == ".pkg":
            verify_macos(package)
        elif package.suffix in (".deb", ".rpm"):
            verify_linux(package)
        else:
            parser.error(f"Unsupported installer format: {package}")


if __name__ == "__main__":
    main()
