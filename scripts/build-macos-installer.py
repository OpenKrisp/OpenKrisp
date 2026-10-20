#!/usr/bin/env python3
"""Build OpenKrisp and its named BlackHole-based audio drivers; never install anything on the build host."""

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
DRIVERS = json.loads((ASSETS / "blackhole.json").read_text())["drivers"]
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


def driver_choice(driver):
    return "needsDriver('%s', '%s')" % (driver["bundle"], driver["identifier"])


def driver_definitions(driver):
    # These are individual argv entries, not shell-escaped commands. Spaces stay inside strings.
    return [
        "-DDEBUG=0", "-DkHas_Driver_Name_Format=false",
        '-DkDriver_Name="%s"' % driver["bundle"],
        '-DkDevice_Name="%s"' % driver["name"],
        '-DkDevice2_Name="%s Mirror"' % driver["name"],
        '-DkPlugIn_BundleID="%s"' % driver["identifier"],
        '-DkPlugIn_Icon="OpenKrisp.icns"',
        "-DkNumber_Of_Channels=%d" % driver["channels"],
    ]


def build_drivers(manifest, source_archive, work, signing_identity):
    source = work / "BlackHole.c"
    with tarfile.open(source_archive, "r:gz") as archive:
        member = archive.extractfile(f"BlackHole-{manifest['version']}/BlackHole/BlackHole.c")
        if member is None:
            raise ValueError("BlackHole source missing.")
        source.write_bytes(member.read())
    for driver in manifest["drivers"]:
        root = work / driver["bundle"]
        bundle = root / (driver["bundle"] + ".driver")
        contents = bundle / "Contents"
        (contents / "MacOS").mkdir(parents=True)
        (contents / "Resources").mkdir()
        info = {
            "CFBundleDevelopmentRegion": "English", "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleExecutable": driver["bundle"], "CFBundleIdentifier": driver["identifier"],
            "CFBundleName": driver["name"], "CFBundlePackageType": "BNDL",
            "CFBundleShortVersionString": manifest["driver_version"],
            "CFBundleVersion": manifest["driver_version"], "CFBundleIconFile": "OpenKrisp.icns",
            "CFPlugInFactories": {driver["factory_uuid"]: "BlackHole_Create"},
            "CFPlugInTypes": {"443ABAB8-E7B3-491A-B985-BEB9187030DB": [driver["factory_uuid"]]},
            "LSMinimumSystemVersion": "10.13",
        }
        with (contents / "Info.plist").open("wb") as stream:
            plistlib.dump(info, stream)
        shutil.copy2(ROOT / "apps/desktop/src-tauri/icons/icon.icns", contents / "Resources/OpenKrisp.icns")
        run("xcrun", "clang", "-bundle", "-O3", "-DNDEBUG", "-std=gnu11",
            "-arch", "arm64", "-arch", "x86_64", "-mmacosx-version-min=10.13",
            # Upstream retains a variadic channel argument when name formatting is disabled.
            "-Wno-format-extra-args",
            *driver_definitions(driver), source,
            "-framework", "CoreAudio", "-framework", "CoreFoundation", "-framework", "Accelerate",
            "-o", contents / "MacOS" / driver["bundle"])
        signing = [] if signing_identity == "-" else ["--timestamp", "--options", "runtime"]
        run("codesign", "--force", "--sign", signing_identity, *signing, bundle)
        run("codesign", "--verify", "--strict", bundle)
        # Fix the path even if a same-ID bundle is found elsewhere. No postinstall daemon restart,
        # broad permission changes, driver deletion, or mutation of existing BlackHole bundles.
        components = work / (driver["bundle"] + "-components.plist")
        run("pkgbuild", "--analyze", "--root", root, components)
        entries = plistlib.loads(components.read_bytes())
        for entry in entries:
            entry["BundleIsRelocatable"] = False
        components.write_bytes(plistlib.dumps(entries))
        run("pkgbuild", "--root", root, "--component-plist", components,
            "--ownership", "recommended", "--identifier", driver["identifier"],
            "--version", manifest["driver_version"], "--install-location", "/Library/Audio/Plug-Ins/HAL",
            work / (driver["bundle"] + ".pkg"))


def build_uninstaller(payload, version, signing_identity):
    bundle = payload / "Applications/Uninstall OpenKrisp.app"
    bundle.parent.mkdir(parents=True, exist_ok=True)
    run("osacompile", "-o", bundle, ASSETS / "uninstall.applescript")
    resources = bundle / "Contents/Resources"
    shutil.copy2(ASSETS / "uninstall.command", resources / "uninstall.command")
    shutil.copy2(ROOT / "apps/desktop/src-tauri/icons/icon.icns", resources / "applet.icns")
    info_path = bundle / "Contents/Info.plist"
    info = plistlib.loads(info_path.read_bytes())
    info.update(CFBundleIdentifier="app.openkrisp.uninstaller",
                CFBundleName="Uninstall OpenKrisp", CFBundleShortVersionString=version)
    info_path.write_bytes(plistlib.dumps(info))
    signing = [] if signing_identity == "-" else ["--timestamp", "--options", "runtime"]
    run("codesign", "--force", "--sign", signing_identity, *signing, bundle)
    run("codesign", "--verify", "--strict", bundle)
    return bundle


def distribution(version, driver_version, minimum_os, architectures):
    root = ET.Element("installer-gui-script", {"minSpecVersion": "2"})
    ET.SubElement(root, "title").text = "OpenKrisp"
    for name in ("welcome", "conclusion"):
        ET.SubElement(root, name, {"file": f"{name}.html", "mime-type": "text/html"})
    ET.SubElement(root, "license", {"file": "Licenses.txt", "mime-type": "text/plain"})
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
    # Never downgrade an independently updated OpenKrisp driver. Leave shared BlackHole intact.
    ET.SubElement(root, "script").text = """
function needsDriver(bundle, identifier) {
    var base = '/Library/Audio/Plug-Ins/HAL/' + bundle + '.driver/Contents/';
    var info = system.files.plistAtPath(base + 'Info.plist');
    return !(info && info.CFBundleIdentifier === identifier &&
        info.CFBundleShortVersionString && info.CFBundleExecutable &&
        system.files.fileExistsAtPath(base + 'MacOS/' + info.CFBundleExecutable) &&
        system.compareVersions(info.CFBundleShortVersionString, %s) >= 0);
}
""" % json.dumps(driver_version)
    outline = ET.SubElement(root, "choices-outline")
    components = [(APP_ID, "OpenKrisp", "OpenKrisp-app.pkg", version, "true")]
    components.extend((driver["identifier"], driver["name"], driver["bundle"] + ".pkg",
                       driver_version, driver_choice(driver)) for driver in DRIVERS)
    for identifier, title, filename, component_version, selected in components:
        ET.SubElement(outline, "line", {"choice": identifier})
        choice = ET.SubElement(root, "choice", {
            "id": identifier, "title": title, "description": title,
            "visible": "false", "enabled": "false",
            "selected": selected,
        })
        ET.SubElement(choice, "pkg-ref", {"id": identifier})
        ET.SubElement(root, "pkg-ref", {
            "id": identifier, "version": component_version,
            "onConclusion": "None" if identifier == APP_ID else "RequireRestart",
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
    if not args.development and not args.community:
        run("spctl", "--assess", "--type", "execute", app)
    manifest = json.loads((ASSETS / "blackhole.json").read_text())
    cache = args.cache.resolve()
    source_archive = verified_download(manifest["source"], cache / "BlackHole-source.tar.gz")
    with tempfile.TemporaryDirectory(prefix="openkrisp-installer-") as temporary:
        work = Path(temporary)
        build_drivers(manifest, source_archive, work,
                      "-" if args.development or args.community else args.driver_identity)
        payload = work / "root"
        project_notices = payload / "Library/Application Support/OpenKrisp"
        project_notices.mkdir(parents=True)
        build_uninstaller(payload, version,
                          "-" if args.development or args.community else args.driver_identity)
        for name in ("LICENSE", "NOTICE"):
            shutil.copy2(ROOT / name, project_notices / name)
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
            "OpenKrisp Microphone and OpenKrisp Speaker are builds of BlackHole 0.7.1.\n"
            "BlackHole is Copyright Existential Audio Inc., licensed under GPL-3.0.\n"
            "The original source is in BlackHole-source.tar.gz. No upstream C source is modified.\n"
            "Names, identities, icon and channel counts are set by the included build script/manifest.\n"
            "Upstream: https://github.com/ExistentialAudio/BlackHole\n"
            "Existing official BlackHole drivers are not modified or removed.\n"
            "See build/scripts/build-macos-installer.py and provenance.json for the build inputs.\n"
        )
        for relative in ["scripts/build-macos-installer.py", "packaging/macos/blackhole.json",
                         "packaging/macos/uninstall.command",
                         "packaging/macos/uninstall.applescript",
                         "packaging/macos/welcome.html", "packaging/macos/conclusion.html",
                         "apps/desktop/src-tauri/icons/icon.icns"]:
            destination = notices / "build" / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, destination)
        resources = work / "resources"
        resources.mkdir()
        for name in ("welcome", "conclusion"):
            shutil.copy2(ASSETS / f"{name}.html", resources / f"{name}.html")
        (resources / "Licenses.txt").write_bytes(
            b"OpenKrisp: GNU General Public License v3.0 only (GPL-3.0-only)\n\n"
            + (ROOT / "NOTICE").read_bytes() + b"\n"
            + (ROOT / "LICENSE").read_bytes()
            + b"\n\nBlackHole audio drivers: separate GPL-3.0 terms\n\n"
            + license_text
        )
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
        distribution(version, manifest["driver_version"], info.get("LSMinimumSystemVersion", "10.13"),
                     architectures).write(definition, encoding="utf-8", xml_declaration=True)
        output.parent.mkdir(parents=True, exist_ok=True)
        signing = [] if args.development or args.community else ["--sign", args.installer_identity]
        run("productbuild", "--distribution", definition, "--resources", resources,
            "--package-path", work, *signing, output)
    print(f"Built {output}")
    if args.development:
        print("Unsigned development installer. Do not distribute as a release.")
    elif args.community:
        print("Community installer: ad-hoc signed app/drivers, unsigned package, not notarized.")
    else:
        print("Notarize and staple this combined package before distribution.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path, default=ROOT / "target/installer-cache")
    parser.add_argument("--driver-identity", help="Developer ID Application certificate for the audio drivers")
    signing = parser.add_mutually_exclusive_group(required=True)
    signing.add_argument("--development", action="store_true", help="Unsigned local test artifact")
    signing.add_argument("--community", action="store_true",
                         help="Community beta: verified app signature, ad-hoc drivers, unsigned package")
    signing.add_argument("--installer-identity", help="Developer ID Installer certificate")
    args = parser.parse_args()
    try:
        if args.driver_identity and (args.development or args.community):
            raise ValueError("--driver-identity is only supported with --installer-identity.")
        if args.installer_identity and not args.driver_identity:
            raise ValueError("Release builds require --driver-identity (Developer ID Application).")
        build(args)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Installer build failed: {error}\n")


if __name__ == "__main__":
    main()
