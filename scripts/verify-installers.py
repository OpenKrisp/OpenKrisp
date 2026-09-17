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
        for variant in ("2ch", "16ch"):
            identifier = f"audio.existential.BlackHole{variant}"
            choice = distribution.find(f"choice[@id='{identifier}']")
            assert choice is not None and choice.get("selected") == f"needsBlackHole('{variant}')"
            driver = distribution.find(f"pkg-ref[@id='{identifier}']")
            assert driver is not None and driver.get("onConclusion") == "RequireRestart"
            component = expanded / f"BlackHole{variant}.pkg"
            driver_metadata = ET.parse(component / "PackageInfo").getroot()
            assert driver_metadata.get("identifier") == identifier
            assert driver_metadata.get("install-location") == "/Library/Audio/Plug-Ins/HAL"
            assert driver_metadata.get("version") == pins["version"]
            with (component / f"Payload/BlackHole{variant}.driver/Contents/Info.plist").open("rb") as stream:
                info = plistlib.load(stream)
            assert info["CFBundleIdentifier"] == identifier
            assert info["CFBundleShortVersionString"] == pins["version"]
        payload = expanded / "OpenKrisp-app.pkg/Payload"
        with (payload / "Applications/OpenKrisp.app/Contents/Info.plist").open("rb") as stream:
            app = plistlib.load(stream)
        assert app["CFBundleIdentifier"] == "app.openkrisp.desktop"
        assert (payload / "Applications/OpenKrisp.app/Contents/MacOS" / app["CFBundleExecutable"]).is_file()
        notices = payload / "Library/Application Support/OpenKrisp/ThirdParty/BlackHole"
        assert (notices / "LICENSE.txt").is_file()
        assert json.loads((notices / "provenance.json").read_text()) == pins
        assert hashlib.sha256((notices / "BlackHole-source.tar.gz").read_bytes()).hexdigest() == pins["source"]["sha256"]
        print("macOS package: app, both drivers, matching source/license and independent restart choices verified.")


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
