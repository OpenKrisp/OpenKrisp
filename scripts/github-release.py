#!/usr/bin/env python3
"""Validate, assemble, stage, and publish releases for the manual Actions workflow."""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
MARKER = "<!-- openkrisp-actions-release-v2 -->"
WORKFLOW = ".github/workflows/release.yml"


def run(*args, cwd=None):
    return subprocess.check_output([str(a) for a in args], cwd=ROOT if cwd is None else cwd, text=True).strip()


def gh(*args):
    return json.loads(run("gh", "api", *args))


def version_for(tag):
    number = r"(?:0|[1-9][0-9]*)"
    if not re.fullmatch(rf"v{number}\.{number}\.{number}(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?", tag):
        raise ValueError("Use a version tag such as v0.1.0-beta.2 (no build metadata).")
    version = tag[1:]
    if "-" in version:
        for part in version.split("-", 1)[1].split("."):
            if part.isdigit() and len(part) > 1 and part.startswith("0"):
                raise ValueError("Numeric prerelease identifiers cannot have leading zeros.")
    return version


def validate_source(tag):
    version = version_for(tag)
    commit = run("git", "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}")
    subprocess.run(["git", "merge-base", "--is-ancestor", commit, "refs/remotes/origin/main"],
                   cwd=ROOT, check=True)

    def text_at(path):
        return run("git", "show", f"{commit}:{path}")

    cargo = tomllib.loads(text_at("Cargo.toml"))
    lock = tomllib.loads(text_at("Cargo.lock"))
    package = json.loads(text_at("apps/desktop/package.json"))
    npm_lock = json.loads(text_at("apps/desktop/package-lock.json"))
    tauri = json.loads(text_at("apps/desktop/src-tauri/tauri.conf.json"))
    versions = [cargo["workspace"]["package"]["version"], package["version"],
                npm_lock["version"], npm_lock["packages"][""]["version"], tauri["version"]]
    workspace_names = set()
    for member in cargo["workspace"]["members"]:
        crate = tomllib.loads(text_at(f"{member}/Cargo.toml"))["package"]
        workspace_names.add(crate["name"])
        if crate["version"] != {"workspace": True}:
            versions.append(crate["version"])
    locked = [p for p in lock["package"] if "source" not in p and p["name"] in workspace_names]
    if {p["name"] for p in locked} != workspace_names:
        raise ValueError("Cargo.lock is missing a workspace package.")
    versions.extend(p["version"] for p in locked)
    if any(v != version for v in versions):
        raise ValueError("Tag, Cargo, npm, lockfile, and Tauri versions must match.")
    for path in [WORKFLOW, "scripts/github-release.py", "packaging/macos/INSTALL.md", "packaging/linux/INSTALL.md",
                 f"docs/releases/{version}.md"]:
        if not text_at(path):
            raise ValueError(f"Missing release input: {path}")
    return version, commit


def remote_commit(repo, tag):
    obj = gh(f"repos/{repo}/git/ref/tags/{tag}")["object"]
    while obj["type"] == "tag":
        obj = gh(f"repos/{repo}/git/tags/{obj['sha']}")["object"]
    if obj["type"] != "commit":
        raise ValueError("Release tag must identify a commit.")
    return obj["sha"]


def successful_ci(repo, commit):
    result = gh(f"repos/{repo}/actions/workflows/ci.yml/runs?head_sha={commit}&event=push&status=success&per_page=100")
    for workflow in result["workflow_runs"]:
        if (workflow["head_sha"] == commit and workflow["head_branch"] == "main"
                and workflow["conclusion"] == "success"):
            return workflow["id"]
    raise ValueError("The tagged commit needs a successful CI push run on main first.")


def release_for(repo, tag):
    result = subprocess.run(["gh", "api", f"repos/{repo}/releases/tags/{tag}"],
                            text=True, capture_output=True, check=False)
    if result.returncode == 0:
        return json.loads(result.stdout)
    if "(HTTP 404)" in result.stderr:
        # The by-tag REST endpoint omits drafts. Authenticated release listings
        # include them, so inspect every page before deciding to create anything.
        pages = gh(f"repos/{repo}/releases?per_page=100", "--paginate", "--slurp")
        matches = [item for page in pages for item in page if item["tag_name"] == tag]
        if len(matches) > 1:
            raise ValueError("Multiple releases use this tag; inspect them manually.")
        return matches[0] if matches else None
    raise RuntimeError(result.stderr)


def owned_draft(release, commit):
    if (not release["draft"] or release.get("immutable")
            or release["target_commitish"] != commit or MARKER not in (release["body"] or "")):
        raise ValueError("Refusing to change a published, unrelated, or manually prepared release.")


def names_for(version):
    return {f"OpenKrisp-{version}-macos-arm64.pkg", f"OpenKrisp-{version}-source.tar.gz",
            f"OpenKrisp-{version}-linux-amd64.deb", f"OpenKrisp-{version}-linux-x86_64.rpm",
            "INSTALL-macos.md", "INSTALL-linux.md", "RELEASE_NOTES.md", "VALIDATION.md", "PROVENANCE.json", "SHA256SUMS"}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_assets(directory, tag, commit):
    expected = names_for(version_for(tag))
    if {p.name for p in directory.iterdir()} != expected:
        raise ValueError("Release assets are missing or contain unexpected files.")
    if any(p.is_symlink() or not p.is_file() for p in directory.iterdir()):
        raise ValueError("Release assets must be regular files.")
    checksums = {}
    for line in (directory / "SHA256SUMS").read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
        if not match or match[2] in checksums:
            raise ValueError("Invalid or duplicate checksum entry.")
        checksums[match[2]] = match[1]
    if set(checksums) != expected - {"SHA256SUMS"}:
        raise ValueError("Checksums must cover exactly the release assets.")
    for name, expected_hash in checksums.items():
        if digest(directory / name) != expected_hash:
            raise ValueError(f"SHA-256 mismatch: {name}")
    provenance = json.loads((directory / "PROVENANCE.json").read_text())
    if (provenance.get("schema") != 2 or provenance.get("tag") != tag
            or provenance.get("source_commit") != commit or provenance.get("operation") != "prepare"
            or provenance.get("workflow") != WORKFLOW):
        raise ValueError("Provenance does not match this workflow, tag, and source commit.")
    return provenance


def package(args):
    version = version_for(args.tag)
    if run("git", "rev-parse", "HEAD") != args.commit or run("git", "status", "--porcelain", "--untracked-files=no"):
        raise ValueError("Package from a clean checkout of the validated source commit.")
    output = args.directory
    inputs = [(args.installer, ".pkg"), (args.deb, ".deb"), (args.rpm, ".rpm")]
    if any(path is None or not path.is_file() or path.is_symlink() or path.suffix != suffix
           for path, suffix in inputs):
        raise ValueError("All three regular installer files (.pkg, .deb, .rpm) are required.")
    if output.exists():
        raise ValueError("Refusing to overwrite an existing asset directory.")
    output.mkdir(parents=True)
    shutil.copyfile(args.installer, output / f"OpenKrisp-{version}-macos-arm64.pkg")
    shutil.copyfile(args.deb, output / f"OpenKrisp-{version}-linux-amd64.deb")
    shutil.copyfile(args.rpm, output / f"OpenKrisp-{version}-linux-x86_64.rpm")
    run("git", "archive", "--format=tar.gz", f"--prefix=OpenKrisp-{version}/",
        "--output", output.resolve() / f"OpenKrisp-{version}-source.tar.gz", args.commit)
    shutil.copyfile(ROOT / "packaging/macos/INSTALL.md", output / "INSTALL-macos.md")
    shutil.copyfile(ROOT / "packaging/linux/INSTALL.md", output / "INSTALL-linux.md")
    shutil.copyfile(ROOT / f"docs/releases/{version}.md", output / "RELEASE_NOTES.md")
    run_id, attempt = int(os.environ["GITHUB_RUN_ID"]), int(os.environ["GITHUB_RUN_ATTEMPT"])
    ci_id = int(os.environ["RELEASE_CI_RUN_ID"])
    repo = os.environ["GITHUB_REPOSITORY"]
    provenance = {
        "schema": 2, "tag": args.tag, "source_commit": args.commit, "operation": "prepare",
        "repository": repo, "workflow": WORKFLOW, "workflow_run_id": run_id,
        "workflow_run_attempt": attempt, "workflow_commit": os.environ["GITHUB_SHA"],
        "ci_run_id": ci_id,
        "targets": {
            "macos-arm64": {"target": "aarch64-apple-darwin", "minimum_os": "11.0"},
            "linux-amd64-deb": {"target": "x86_64-unknown-linux-gnu", "build_os": "Debian 12"},
            "linux-x86_64-rpm": {"target": "x86_64-unknown-linux-gnu", "build_os": "Fedora 44"},
        },
        "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "signing": {"macos_app": "ad-hoc", "macos_drivers": "ad-hoc", "packages": "unsigned", "notarized": False},
    }
    (output / "PROVENANCE.json").write_text(json.dumps(provenance, indent=2) + "\n")
    (output / "VALIDATION.md").write_text(
        f"# Validation for {args.tag}\n\nSource commit: `{args.commit}`.\n\n"
        f"- [Routine CI](https://github.com/{repo}/actions/runs/{ci_id}) passed for this exact commit.\n"
        f"- [Release preparation](https://github.com/{repo}/actions/runs/{run_id}/attempts/{attempt}) "
        "built the optimized macOS ARM64 app, verified its signature, ran Python tests and both release "
        "performance benchmarks, and inspected the installer with native driver probes.\n"
        "- Native Linux x86-64 jobs built DEB on Debian 12 and RPM on Fedora 44, checked "
        "package architectures/versions, runtime dependencies and bundled notices, and ran Python tests.\n"
        "- Disposable Debian 12, Ubuntu 24.04, and Fedora 44 containers installed the actual packages "
        "through APT/DNF and checked the installed executable's dynamic libraries.\n\n"
        "Fresh-Mac download/Gatekeeper, system installation, upgrade/restart, microphone permission, "
        "real-call listening, and long-run device acceptance are not performed by this workflow. "
        "Linux desktop GUI, PipeWire session routing, real hardware/calls, and long-run behavior "
        "are not qualified by package installation checks. macOS Intel and Linux ARM binaries "
        "are not included. Packages are unsigned; the macOS installer is not notarized.\n"
    )
    (output / "SHA256SUMS").write_text("".join(
        f"{digest(path)}  {path.name}\n" for path in sorted(output.iterdir())))
    verify_assets(output, args.tag, args.commit)


def verify_uploaded(release, directory):
    assets = {a["name"]: a for a in release["assets"]}
    if set(assets) != {p.name for p in directory.iterdir()}:
        raise ValueError("Uploaded assets do not match the prepared file set.")
    for path in directory.iterdir():
        asset = assets[path.name]
        if (asset["state"] != "uploaded" or asset["size"] != path.stat().st_size
                or asset.get("digest") != "sha256:" + digest(path)):
            raise ValueError(f"Uploaded asset failed verification: {path.name}")


def prepare(args, repo):
    provenance = verify_assets(args.directory, args.tag, args.commit)
    if (provenance["repository"] != repo
            or provenance["workflow_run_id"] != int(os.environ["GITHUB_RUN_ID"])
            or provenance["workflow_run_attempt"] != int(os.environ["GITHUB_RUN_ATTEMPT"])
            or provenance["workflow_commit"] != os.environ["GITHUB_SHA"]):
        raise ValueError("Assets must come from this preparation run.")
    if remote_commit(repo, args.tag) != args.commit:
        raise ValueError("Tag moved after validation.")
    release = release_for(repo, args.tag)
    with tempfile.TemporaryDirectory() as temp:
        notes = Path(temp) / "notes.md"
        notes.write_text((args.directory / "RELEASE_NOTES.md").read_text()
                         + f"\n\nSource commit: `{args.commit}`.\n\n{MARKER}\n")
        if release:
            owned_draft(release, args.commit)
            if set(a["name"] for a in release["assets"]) - names_for(version_for(args.tag)):
                raise ValueError("Existing draft contains unexpected assets; inspect it manually.")
            run("gh", "release", "edit", args.tag, "--repo", repo, "--notes-file", notes)
        else:
            flags = ["--prerelease"] if "-" in version_for(args.tag) else []
            run("gh", "release", "create", args.tag, "--repo", repo, "--verify-tag", "--draft",
                "--target", args.commit, "--latest=false", "--title", f"OpenKrisp {version_for(args.tag)}",
                "--notes-file", notes, *flags)
    run("gh", "release", "upload", args.tag, "--repo", repo, "--clobber", *sorted(args.directory.iterdir()))
    release = release_for(repo, args.tag)
    owned_draft(release, args.commit)
    verify_uploaded(release, args.directory)
    print(f"Verified draft: {release['html_url']}")


def publish(args, repo):
    release = release_for(repo, args.tag)
    if release is None:
        raise ValueError("Run prepare first; no draft exists for this tag.")
    owned_draft(release, args.commit)
    with tempfile.TemporaryDirectory() as temp:
        downloaded = Path(temp) / "release"
        run("gh", "release", "download", args.tag, "--repo", repo, "--dir", downloaded)
        provenance = verify_assets(downloaded, args.tag, args.commit)
        if provenance["repository"] != repo:
            raise ValueError("Draft was prepared for a different repository.")
        run_id = int(provenance["workflow_run_id"])
        attempt = int(provenance["workflow_run_attempt"])
        build = gh(f"repos/{repo}/actions/runs/{run_id}/attempts/{attempt}")
        if (build["conclusion"] != "success" or build["event"] != "workflow_dispatch"
                or build["path"] != WORKFLOW or build["head_branch"] != "main"
                or build["head_sha"] != provenance["workflow_commit"]):
            raise ValueError("Draft needs a successful prepare run of the release workflow on main.")
        original = Path(temp) / "original"
        run("gh", "run", "download", str(run_id), "--repo", repo,
            "--name", f"community-{args.tag}-{attempt}", "--dir", original)
        verify_assets(original, args.tag, args.commit)
        if any(digest(path) != digest(original / path.name) for path in downloaded.iterdir()):
            raise ValueError("Draft assets differ from the retained prepare-run artifact.")
        if remote_commit(repo, args.tag) != args.commit:
            raise ValueError("Tag moved after preparation.")
        release = release_for(repo, args.tag)
        owned_draft(release, args.commit)
        verify_uploaded(release, downloaded)
        run("gh", "release", "edit", args.tag, "--repo", repo, "--draft=false", "--latest=false",
            f"--prerelease={'true' if '-' in version_for(args.tag) else 'false'}")
        published = release_for(repo, args.tag)
        if published["draft"]:
            raise ValueError("Release remained a draft.")
        verify_uploaded(published, downloaded)
        print(f"Published: {published['html_url']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["validate", "package", "prepare", "publish"])
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit")
    parser.add_argument("--directory", type=Path, default=ROOT / "target/release-assets")
    parser.add_argument("--installer", type=Path)
    parser.add_argument("--deb", type=Path)
    parser.add_argument("--rpm", type=Path)
    args = parser.parse_args()
    version_for(args.tag)
    if args.operation == "package":
        if not args.commit or not args.installer or not args.deb or not args.rpm:
            parser.error("package requires --commit, --installer, --deb, and --rpm")
        package(args)
        return
    repo = os.environ["GITHUB_REPOSITORY"]
    if os.environ.get("GITHUB_REF") != "refs/heads/main":
        raise ValueError("Run the release workflow from main.")
    if args.operation == "validate":
        version, commit = validate_source(args.tag)
        if remote_commit(repo, args.tag) != commit:
            raise ValueError("Tag moved after checkout.")
        ci_run = successful_ci(repo, commit)
        existing = release_for(repo, args.tag)
        if existing:
            owned_draft(existing, commit)
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(f"version={version}\ncommit={commit}\nci_run={ci_run}\n")
    elif not args.commit or not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        parser.error("prepare/publish require a full validated --commit")
    elif args.operation == "prepare":
        prepare(args, repo)
    else:
        publish(args, repo)


if __name__ == "__main__":
    main()
