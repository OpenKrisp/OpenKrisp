#!/usr/bin/env python3
"""Build a combined app/BlackHole installer; never install anything on the build host."""

import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "packaging/macos"
BLACKHOLE_ID = "audio.existential.BlackHole2ch"
APP_ID = "app.openkrisp.desktop"


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def verified_download(spec, destination):
    if not destination.exists():
        destination.parent.mkdir(parents=True, exist_ok=True)
        # Only rename after verification; interrupted downloads cannot become cache hits.
        with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
            partial = Path(temporary) / "download"
            run("curl", "--fail", "--location", "--proto", "=https",
                "--proto-redir", "=https", "--tlsv1.2", "--retry", "3",
                "--connect-timeout", "30", "--max-time", "300",
                "--output", partial, spec["url"])
            verify_hash(partial, spec["sha256"])
            partial.replace(destination)
    verify_hash(destination, spec["sha256"])
    return destination


def verify_hash(path, expected):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    if digest.hexdigest() != expected:
        raise ValueError(f"SHA-256 mismatch: {path}. Remove the file and retry.")


def distribution(version, blackhole_version, minimum_os, architectures):
    root = ET.Element("installer-gui-script", {"minSpecVersion": "2"})
    ET.SubElement(root, "title").text = "OpenKrisp"
    for name in ("welcome", "conclusion"):
        ET.SubElement(root, name, {"file": f"{name}.html", "mime-type": "text/html"})
    ET.SubElement(root, "license", {"file": "BlackHole-LICENSE.txt", "mime-type": "text/plain"})
    ET.SubElement(root, "domains", {
        "enable_anywhere": "false", "enable_currentUserHome": "false",
        "enable_localSystem": "true",
    })
    ET.SubElement(root, "options", {
        "customize": "never", "require-scripts": "true",
        "hostArchitectures": ",".join(architectures),
    })
    allowed = ET.SubElement(ET.SubElement(root, "volume-check"), "allowed-os-versions")
    ET.SubElement(allowed, "os-version", {"min": minimum_os})
    # Check the installed bundle, not just receipts: deleting the driver must permit repair.
    # Never downgrade an independently updated BlackHole installation.
    ET.SubElement(root, "script").text = """
function needsBlackHole() {
    var info = system.files.plistAtPath('/Library/Audio/Plug-Ins/HAL/BlackHole2ch.driver/Contents/Info.plist');
    return !(info && info.CFBundleIdentifier === 'audio.existential.BlackHole2ch' &&
        info.CFBundleShortVersionString &&
        system.compareVersions(info.CFBundleShortVersionString, %s) >= 0);
}
""" % json.dumps(blackhole_version)
    outline = ET.SubElement(root, "choices-outline")
    for identifier, title, filename, component_version in (
        (APP_ID, "OpenKrisp", "OpenKrisp-app.pkg", version),
        (BLACKHOLE_ID, "BlackHole 2ch", "BlackHole.pkg", blackhole_version),
    ):
        ET.SubElement(outline, "line", {"choice": identifier})
        choice = ET.SubElement(root, "choice", {
            "id": identifier, "title": title, "description": title,
            "visible": "false", "enabled": "false",
            "selected": "needsBlackHole()" if identifier == BLACKHOLE_ID else "true",
        })
        ET.SubElement(choice, "pkg-ref", {"id": identifier})
        ET.SubElement(root, "pkg-ref", {
            "id": identifier, "version": component_version,
            "onConclusion": "RequireRestart" if identifier == BLACKHOLE_ID else "None",
        }).text = filename
    return ET.ElementTree(root)


def build(args):
    if sys.platform != "darwin":
        raise ValueError("Build the macOS installer on macOS.")
    app = args.app.resolve()
    output = args.output.resolve()
    if output.exists():
        raise ValueError(f"Refusing to overwrite {output}")
    with (app / "Contents/Info.plist").open("rb") as source:
        info = plistlib.load(source)
    if app.name != "OpenKrisp.app" or info["CFBundleIdentifier"] != APP_ID:
        raise ValueError("Expected the Tauri OpenKrisp.app bundle.")
    version = info["CFBundleShortVersionString"]
    executable = app / "Contents/MacOS" / info["CFBundleExecutable"]
    architectures = run("lipo", "-archs", executable, capture_output=True,
                        text=True).stdout.split()
    if not architectures or set(architectures) - {"arm64", "x86_64"}:
        raise ValueError(f"Unexpected app architectures: {architectures}")
    if not args.development:
        run("codesign", "--verify", "--deep", "--strict", app)
        run("spctl", "--assess", "--type", "execute", app)
    manifest = json.loads((ASSETS / "blackhole.json").read_text())
    cache = args.cache.resolve()
    driver_pkg = verified_download(manifest["package"], cache / "BlackHole2ch.pkg")
    source_archive = verified_download(manifest["source"], cache / "BlackHole-source.tar.gz")
    # Release builds additionally require Apple's trust assessment of the upstream package.
    # Development builds remain hash-pinned and work without keychain/network trust services.
    if not args.development:
        run("pkgutil", "--check-signature", driver_pkg)
        run("spctl", "--assess", "--type", "install", driver_pkg)
    with tempfile.TemporaryDirectory(prefix="openkrisp-installer-") as temporary:
        work = Path(temporary)
        expanded = work / "blackhole"
        run("pkgutil", "--expand", driver_pkg, expanded)
        component = expanded / "BlackHole.pkg"
        metadata = ET.parse(component / "PackageInfo").getroot()
        if (metadata.get("identifier") != BLACKHOLE_ID
                or metadata.get("version") != manifest["version"]
                or metadata.get("install-location") != "/Library/Audio/Plug-Ins/HAL"):
            raise ValueError("Unexpected BlackHole package metadata.")
        # Repack the verified upstream component without altering its payload or scripts.
        run("pkgutil", "--flatten", component, work / "BlackHole.pkg")
        payload = work / "root"
        bundled_app = payload / "Applications/OpenKrisp.app"
        shutil.copytree(app, bundled_app, symlinks=True)
        notices = payload / "Library/Application Support/OpenKrisp/ThirdParty/BlackHole"
        notices.mkdir(parents=True)
        shutil.copy2(source_archive, notices / "BlackHole-source.tar.gz")
        shutil.copy2(ASSETS / "blackhole.json", notices / "provenance.json")
        with tarfile.open(source_archive, "r:gz") as archive:
            license_file = archive.extractfile(f"BlackHole-{manifest['version']}/LICENSE")
            if license_file is None:
                raise ValueError("BlackHole source archive has no license.")
            license_text = license_file.read()
        (notices / "LICENSE.txt").write_bytes(license_text)
        (notices / "README.txt").write_text(
            "BlackHole 2ch is Copyright Existential Audio Inc., licensed under GPL-3.0.\n"
            "The matching upstream source and build scripts are in BlackHole-source.tar.gz.\n"
            "The installer reuses the official driver payload and installation scripts.\n"
            "Upstream: https://github.com/ExistentialAudio/BlackHole\n"
            "BlackHole is shared with other apps; deleting OpenKrisp does not remove it.\n"
        )
        resources = work / "resources"
        resources.mkdir()
        for name in ("welcome", "conclusion"):
            shutil.copy2(ASSETS / f"{name}.html", resources / f"{name}.html")
        (resources / "BlackHole-LICENSE.txt").write_bytes(license_text)
        components = work / "components.plist"
        run("pkgbuild", "--analyze", "--root", payload, components)
        with components.open("rb") as source:
            entries = plistlib.load(source)
        for entry in entries:
            entry["BundleIsRelocatable"] = False
        with components.open("wb") as target:
            plistlib.dump(entries, target)
        run("pkgbuild", "--root", payload, "--component-plist", components,
            "--identifier", APP_ID, "--version", version, "--install-location", "/",
            work / "OpenKrisp-app.pkg")
        definition = work / "Distribution.xml"
        distribution(version, manifest["version"], info.get("LSMinimumSystemVersion", "10.13"),
                     architectures).write(definition, encoding="utf-8", xml_declaration=True)
        output.parent.mkdir(parents=True, exist_ok=True)
        signing = [] if args.development else ["--sign", args.installer_identity]
        run("productbuild", "--distribution", definition, "--resources", resources,
            "--package-path", work, *signing, output)
    print(f"Built {output}")
    if args.development:
        print("Unsigned development installer. Do not distribute as a release.")
    else:
        print("Notarize and staple this combined package before distribution.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path, default=ROOT / "target/installer-cache")
    signing = parser.add_mutually_exclusive_group(required=True)
    signing.add_argument("--development", action="store_true", help="Unsigned local test artifact")
    signing.add_argument("--installer-identity", help="Developer ID Installer certificate")
    args = parser.parse_args()
    try:
        build(args)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Installer build failed: {error}\n")


if __name__ == "__main__":
    main()
