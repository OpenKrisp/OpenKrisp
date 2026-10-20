#!/usr/bin/env python3
"""Collect pinned dependency notices and MPL crate sources for a desktop release."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
LEGAL_NAME = re.compile(r"^(licen[cs]es?|copying|copyright|notices?|authors|patents|unlicense)([-_.].*)?$", re.I)


def json_command(*args, cwd=ROOT):
    return json.loads(subprocess.check_output(args, cwd=cwd, text=True))


def legal_files(base):
    result = []
    for path in base.iterdir():
        if not LEGAL_NAME.match(path.name):
            continue
        if path.is_file():
            result.append(path)
        elif path.is_dir():
            result.extend(p for p in path.rglob("*") if p.is_file())
    return sorted(result)


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "OpenKrisp-release-notices"})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read()


def upstream_notices(package, base, allow_fetch):
    """Some published crates omit their repository's top-level license files."""
    vcs = json.loads((base / ".cargo_vcs_info.json").read_text())
    revision = vcs["git"]["sha1"]
    repository = (package.get("repository") or "").removesuffix("/").removesuffix(".git")
    match = re.fullmatch(r"https://github.com/([\w.-]+)/([\w.-]+)", repository)
    if not match or not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError(f"Cannot resolve pinned upstream notices for {package['name']}")
    owner, repo = match.groups()
    cache = ROOT / "target/license-cache" / f"{owner}-{repo}-{revision}"
    record = cache / "provenance.json"
    if not record.exists():
        if not allow_fetch:
            raise ValueError(f"Missing upstream notices for {package['name']}; rerun with --fetch-missing")
        tree = json.loads(fetch(f"https://api.github.com/repos/{owner}/{repo}/git/trees/{revision}"))
        entries = [entry for entry in tree["tree"]
                   if entry["type"] == "blob" and LEGAL_NAME.match(entry["path"])]
        for directory in tree["tree"]:
            if directory["type"] != "tree" or not LEGAL_NAME.match(directory["path"]):
                continue
            children = json.loads(fetch(
                f"https://api.github.com/repos/{owner}/{repo}/git/trees/{directory['sha']}?recursive=1"))
            if children.get("truncated"):
                raise ValueError(f"Incomplete upstream license directory for {package['name']}")
            entries.extend({**entry, "path": directory["path"] + "/" + entry["path"]}
                           for entry in children["tree"] if entry["type"] == "blob")
        if not entries:
            raise ValueError(f"No license files or license directories at {repository}/tree/{revision}")
        cache.mkdir(parents=True, exist_ok=True)
        sources = []
        for entry in entries:
            name = entry["path"]
            if Path(name).is_absolute() or ".." in Path(name).parts:
                raise ValueError("Unsafe upstream license path")
            url = f"https://raw.githubusercontent.com/{owner}/{repo}/{revision}/{name}"
            data = fetch(url)
            (cache / name).parent.mkdir(parents=True, exist_ok=True)
            (cache / name).write_bytes(data)
            sources.append({"file": name, "url": url, "sha256": hashlib.sha256(data).hexdigest()})
        record.write_text(json.dumps(sources, indent=2) + "\n")
    sources = json.loads(record.read_text())
    for source in sources:
        data = (cache / source["file"]).read_bytes()
        if hashlib.sha256(data).hexdigest() != source["sha256"]:
            raise ValueError(f"Corrupt cached notice: {source['file']}")
    return cache, legal_files(cache), sources


def collect(args):
    destination = args.output.resolve()
    if destination.exists():
        raise ValueError(f"Refusing to overwrite {destination}")
    metadata = json_command("cargo", "metadata", "--format-version", "1", "--locked",
                            "--offline", "--filter-platform", args.target)
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    package_id = next(p["id"] for p in metadata["packages"] if p["name"] == "openkrisp-desktop")
    pending, selected = [package_id], set()
    while pending:
        identifier = pending.pop()
        if identifier in selected:
            continue
        selected.add(identifier)
        pending.extend(dep["pkg"] for dep in nodes[identifier]["deps"]
                       if any(kind["kind"] != "dev" for kind in dep["dep_kinds"]))

    packages = sorted((p for p in metadata["packages"] if p["id"] in selected and p["source"]),
                      key=lambda p: (p["name"], p["version"]))
    # Stage separately so a failed collection cannot be bundled as a complete inventory.
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="openkrisp-notices-", dir=destination.parent) as temporary:
        staging = Path(temporary) / "notices"
        staging.mkdir()
        inventory = []
        for package in packages:
            base = Path(package["manifest_path"]).parent
            files = legal_files(base)
            sources = []
            original_base = base
            notice_origin = None
            if not files and (package["name"], package["version"]) == ("libappindicator-sys", "0.9.0"):
                # The sys crate omits repository metadata and the workspace's licenses.
                parent = next(p for p in packages if (p["name"], p["version"]) == ("libappindicator", "0.9.0"))
                parent_base = Path(parent["manifest_path"]).parent
                child_vcs = json.loads((base / ".cargo_vcs_info.json").read_text())
                parent_vcs = json.loads((parent_base / ".cargo_vcs_info.json").read_text())
                if (child_vcs["git"]["sha1"] != parent_vcs["git"]["sha1"]
                        or parent_vcs["path_in_vcs"] != "" or parent["license"] != package["license"]):
                    raise ValueError("libappindicator workspace license provenance does not match")
                base, files = parent_base, legal_files(parent_base)
                notice_origin = "Workspace licenses from libappindicator 0.9.0 at the same pinned Git revision"
            metadata_only_mit = (package["name"], package["version"], package["license"]) == (
                "realfft", "3.5.0", "MIT")
            header_only_mpl = (package["name"], package["version"], package["license"]) == (
                "selectors", "0.36.1", "MPL-2.0")
            if not files and metadata_only_mit:
                # This exact upstream version declares MIT but supplies no license file.
                # Preserve its original declarations and complete source, and add the
                # standard terms without inventing an upstream copyright notice.
                if "License: MIT" not in (base / "README.md").read_text():
                    raise ValueError("realfft README is missing its expected MIT license declaration")
                files = [base / "Cargo.toml", base / "README.md"]
            elif not files and header_only_mpl:
                if "Mozilla Public" not in (base / "nth_index_cache.rs").read_text():
                    raise ValueError("selectors source is missing its expected MPL license declaration")
                files = [base / "Cargo.toml"]
            elif not files:
                base, files, sources = upstream_notices(package, base, args.fetch_missing)
            entry = {"ecosystem": "cargo", "name": package["name"], "version": package["version"],
                     "license": package["license"], "repository": package["repository"],
                     "authors": package["authors"], "upstream_notices": sources}
            if notice_origin:
                entry["notice_origin"] = notice_origin
            folder = staging / "cargo" / f"{package['name']}-{package['version']}"
            for source in files:
                target = folder / source.relative_to(base)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
            if metadata_only_mit:
                shutil.copyfile(ROOT / "packaging/licenses/MIT-standard.txt", folder / "MIT-standard.txt")
                entry["notice_origin"] = "MIT declaration in upstream manifest/README; standard terms supplied separately"
            if header_only_mpl:
                shutil.copyfile(ROOT / "packaging/licenses/MPL-2.0.txt", folder / "MPL-2.0.txt")
                entry["notice_origin"] = "MPL file headers retained in source archive; standard terms supplied separately"
            if "MPL-2.0" in (package["license"] or "") or metadata_only_mit:
                archive = (original_base.parents[2] / "cache" / original_base.parent.name
                           / f"{package['name']}-{package['version']}.crate")
                if not archive.is_file():
                    raise ValueError(f"Missing corresponding source archive: {package['name']}")
                target = folder / archive.name
                shutil.copyfile(archive, target)
                entry["source_archive"] = str(target.relative_to(staging))
                entry["source_sha256"] = hashlib.sha256(target.read_bytes()).hexdigest()
            inventory.append(entry)

        npm_paths = subprocess.check_output(
            ["npm", "ls", "--omit=dev", "--all", "--parseable"], cwd=ROOT / "apps/desktop", text=True)
        for line in sorted(set(npm_paths.splitlines())):
            base = Path(line)
            if base.resolve() == ROOT / "apps/desktop":
                continue
            package = json.loads((base / "package.json").read_text())
            files = legal_files(base)
            if not files:
                raise ValueError(f"Missing npm notices: {package['name']}")
            folder = staging / "npm" / f"{package['name'].replace('/', '__')}-{package['version']}"
            for source in files:
                target = folder / source.relative_to(base)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
            inventory.append({"ecosystem": "npm", "name": package["name"], "version": package["version"],
                              "license": package.get("license")})
        (staging / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
        (staging / "README.txt").write_text(
            "OpenKrisp dependency notices\n\n"
            "This inventory includes Rust runtime and build dependencies for " + args.target
            + " and installed production npm dependencies. Some build-only components are\n"
            "listed conservatively. Each component retains its own license. License\n"
            "expressions describe upstream choices; OpenKrisp's GPLv3 project license\n"
            "does not replace those component terms. Unmodified MPL-2.0 crate source archives\n"
            "are included alongside their notices; inventory.json records their paths.\n"
            "Model, native AEC, and BlackHole notices are shipped separately.\n"
        )
        staging.rename(destination)
    print(f"Prepared {len(inventory)} dependency notices in {destination}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "target/release-notices")
    parser.add_argument("--fetch-missing", action="store_true")
    options = parser.parse_args()
    collect(options)
