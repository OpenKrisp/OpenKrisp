#!/usr/bin/env python3
"""Validate, assemble, stage, and publish versioned Actions releases."""

import argparse
import base64
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
    return validate_commit(version, commit)


def validate_commit(version, commit):
    version_for("v" + version)
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("Expected a full source commit")
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


def outputs(**values):
    with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
        for name, value in values.items():
            stream.write(f"{name}={value}\n")


def triggered_run(repo, event, workflow, trigger):
    """Only successful runs of trusted main workflows can authorize a release."""
    payload = event["workflow_run"]
    identifier, attempt = int(payload["id"]), int(payload["run_attempt"])
    record = gh(f"repos/{repo}/actions/runs/{identifier}/attempts/{attempt}")
    if (record["id"] != identifier or record["run_attempt"] != attempt
            or record["head_sha"] != payload["head_sha"]
            or record["conclusion"] != "success" or record["event"] != trigger
            or record["path"] != workflow or record["head_branch"] != "main"
            or record["head_repository"]["full_name"] != repo):
        raise ValueError("Automatic releases require a successful trusted main workflow run")
    return record


def tag_commit_or_none(repo, tag):
    result = subprocess.run(["gh", "api", f"repos/{repo}/git/ref/tags/{tag}"],
                            text=True, capture_output=True, check=False)
    if result.returncode == 0:
        return remote_commit(repo, tag)
    if "(HTTP 404)" in result.stderr:
        return None
    raise RuntimeError(result.stderr)


def automatic_plan(repo, event):
    ci = triggered_run(repo, event, ".github/workflows/ci.yml", "push")
    commit = ci["head_sha"]
    current = tomllib.loads(run("git", "show", f"{commit}:Cargo.toml"))["workspace"]["package"]["version"]
    parents = run("git", "rev-list", "--parents", "-n", "1", commit).split()[1:]
    if not parents:
        print("No preceding main version; skip automatic release.")
        return outputs(ready="false")
    previous = tomllib.loads(run("git", "show", f"{parents[0]}:Cargo.toml"))["workspace"]["package"]["version"]
    if current == previous:
        print("Version unchanged; skip automatic release.")
        return outputs(ready="false")
    version, commit = validate_commit(current, commit)
    tag = "v" + version
    existing_commit = tag_commit_or_none(repo, tag)
    if existing_commit is not None and existing_commit != commit:
        raise ValueError("Version tag already belongs to a different commit; choose a new version")
    existing = release_for(repo, tag)
    if existing:
        if not existing["draft"] and existing_commit == commit:
            print("Version already published; skip automatic release.")
            return outputs(ready="false")
        owned_draft(existing, commit)
    outputs(ready="true", automatic="true", operation="prepare", tag=tag,
            version=version, commit=commit, ci_run=ci["id"])


def create_tag(repo, tag, commit):
    validate_commit(version_for(tag), commit)
    successful_ci(repo, commit)
    existing = tag_commit_or_none(repo, tag)
    if existing == commit:
        print("Version tag already identifies the tested commit.")
        return
    if existing is not None:
        raise ValueError("Refusing to move an existing version tag")
    gh(f"repos/{repo}/git/refs", "--method", "POST", "-f", f"ref=refs/tags/{tag}", "-f", f"sha={commit}")
    if remote_commit(repo, tag) != commit:
        raise ValueError("Created tag does not match the tested commit")


def automatic_preparation(repo, event):
    record = triggered_run(repo, event, WORKFLOW, "workflow_run")
    identifier, attempt = record["id"], record["run_attempt"]
    pages = gh(f"repos/{repo}/actions/runs/{identifier}/artifacts?per_page=100", "--paginate", "--slurp")
    artifacts = [item for page in pages for item in page["artifacts"]
                 if item["name"].startswith("community-") and item["name"].endswith(f"-{attempt}")]
    if not artifacts:
        print("No release artifact for this attempt; no version was prepared.")
        return None
    if len(artifacts) != 1 or artifacts[0]["expired"]:
        raise ValueError("Expected one unexpired preparation artifact")
    tag = artifacts[0]["name"][len("community-"):-len(f"-{attempt}")]
    version_for(tag)
    return tag, identifier, attempt


def automatic_publish(repo, event):
    preparation = automatic_preparation(repo, event)
    if preparation is None:
        return
    tag, identifier, attempt = preparation
    args = argparse.Namespace(tag=tag, commit=remote_commit(repo, tag))
    publish(args, repo, expected_preparation=(identifier, attempt))


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


def names_for(version, updater=True):
    names = {f"OpenKrisp-{version}-macos-arm64.pkg", f"OpenKrisp-{version}-source.tar.gz",
            f"OpenKrisp-{version}-linux-amd64.deb", f"OpenKrisp-{version}-linux-x86_64.rpm",
            "INSTALL-macos.md", "INSTALL-linux.md", "RELEASE_NOTES.md", "VALIDATION.md", "PROVENANCE.json", "SHA256SUMS"}
    if updater:
        names.update({f"OpenKrisp-{version}-macos-arm64.app.tar.gz",
                      f"OpenKrisp-{version}-macos-arm64.app.tar.gz.sig",
                      f"OpenKrisp-{version}-linux-amd64.deb.sig",
                      f"OpenKrisp-{version}-linux-x86_64.rpm.sig", "latest.json"})
    return names


def updater_manifest(directory, version, repo):
    platforms = {}
    for target, name in (("darwin-aarch64", f"OpenKrisp-{version}-macos-arm64.app.tar.gz"),
                         ("linux-x86_64-deb", f"OpenKrisp-{version}-linux-amd64.deb"),
                         ("linux-x86_64-rpm", f"OpenKrisp-{version}-linux-x86_64.rpm")):
        signature = (directory / (name + ".sig")).read_text().strip()
        try:
            decoded = base64.b64decode(signature, validate=True).decode("utf-8")
            lines = decoded.splitlines()
            if (len(lines) != 4 or not lines[0].startswith("untrusted comment:")
                    or not lines[2].startswith("trusted comment:")
                    or len(base64.b64decode(lines[1], validate=True)) != 74
                    or len(base64.b64decode(lines[3], validate=True)) != 64):
                raise ValueError("Invalid minisign payload")
        except (ValueError, UnicodeError) as error:
            raise ValueError(f"Invalid updater signature: {name}") from error
        platforms[target] = {"url": f"https://github.com/{repo}/releases/download/v{version}/{name}",
                             "signature": signature}
    return {"version": version, "notes": (directory / "RELEASE_NOTES.md").read_text(), "platforms": platforms}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_assets(directory, tag, commit):
    provenance_path = directory / "PROVENANCE.json"
    if not provenance_path.is_file() or provenance_path.is_symlink():
        raise ValueError("Release assets are missing provenance.")
    provenance = json.loads(provenance_path.read_text())
    expected = names_for(version_for(tag), updater=provenance.get("schema") == 3)
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
    if (provenance.get("schema") not in (2, 3) or provenance.get("tag") != tag
            or provenance.get("source_commit") != commit or provenance.get("operation") != "prepare"
            or provenance.get("workflow") != WORKFLOW):
        raise ValueError("Provenance does not match this workflow, tag, and source commit.")
    if provenance["schema"] == 3:
        manifest = json.loads((directory / "latest.json").read_text())
        if manifest != updater_manifest(directory, version_for(tag), provenance["repository"]):
            raise ValueError("Updater manifest does not match the verified release files.")
    return provenance


def package(args):
    version = version_for(args.tag)
    if run("git", "rev-parse", "HEAD") != args.commit or run("git", "status", "--porcelain", "--untracked-files=no"):
        raise ValueError("Package from a clean checkout of the validated source commit.")
    output = args.directory
    inputs = [(args.installer, ".pkg"), (args.deb, ".deb"), (args.rpm, ".rpm"),
              (args.macos_update, ".gz"), (args.macos_signature, ".sig"),
              (args.deb_signature, ".sig"), (args.rpm_signature, ".sig")]
    if any(path is None or not path.is_file() or path.is_symlink() or path.suffix != suffix
           for path, suffix in inputs):
        raise ValueError("All installers, the macOS app archive, and updater signatures are required.")
    if output.exists():
        raise ValueError("Refusing to overwrite an existing asset directory.")
    output.mkdir(parents=True)
    shutil.copyfile(args.installer, output / f"OpenKrisp-{version}-macos-arm64.pkg")
    shutil.copyfile(args.deb, output / f"OpenKrisp-{version}-linux-amd64.deb")
    shutil.copyfile(args.rpm, output / f"OpenKrisp-{version}-linux-x86_64.rpm")
    for source, name in ((args.macos_update, f"OpenKrisp-{version}-macos-arm64.app.tar.gz"),
                         (args.macos_signature, f"OpenKrisp-{version}-macos-arm64.app.tar.gz.sig"),
                         (args.deb_signature, f"OpenKrisp-{version}-linux-amd64.deb.sig"),
                         (args.rpm_signature, f"OpenKrisp-{version}-linux-x86_64.rpm.sig")):
        shutil.copyfile(source, output / name)
    run("git", "archive", "--format=tar.gz", f"--prefix=OpenKrisp-{version}/",
        "--output", output.resolve() / f"OpenKrisp-{version}-source.tar.gz", args.commit)
    shutil.copyfile(ROOT / "packaging/macos/INSTALL.md", output / "INSTALL-macos.md")
    shutil.copyfile(ROOT / "packaging/linux/INSTALL.md", output / "INSTALL-linux.md")
    shutil.copyfile(ROOT / f"docs/releases/{version}.md", output / "RELEASE_NOTES.md")
    run_id, attempt = int(os.environ["GITHUB_RUN_ID"]), int(os.environ["GITHUB_RUN_ATTEMPT"])
    ci_id = int(os.environ["RELEASE_CI_RUN_ID"])
    repo = os.environ["GITHUB_REPOSITORY"]
    provenance = {
        "schema": 3, "tag": args.tag, "source_commit": args.commit, "operation": "prepare",
        "repository": repo, "workflow": WORKFLOW, "workflow_run_id": run_id,
        "workflow_run_attempt": attempt, "workflow_commit": os.environ["GITHUB_SHA"],
        "ci_run_id": ci_id,
        "trigger": os.environ.get("GITHUB_EVENT_NAME", "workflow_dispatch"),
        "targets": {
            "macos-arm64": {"target": "aarch64-apple-darwin", "minimum_os": "11.0"},
            "linux-amd64-deb": {"target": "x86_64-unknown-linux-gnu", "build_os": "Debian 12"},
            "linux-x86_64-rpm": {"target": "x86_64-unknown-linux-gnu", "build_os": "Fedora 44"},
        },
        "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "signing": {"macos_app": "ad-hoc", "macos_drivers": "ad-hoc", "packages": "unsigned",
                    "updater_artifacts": "minisign", "notarized": False},
    }
    (output / "PROVENANCE.json").write_text(json.dumps(provenance, indent=2) + "\n")
    (output / "latest.json").write_text(json.dumps(updater_manifest(output, version, repo), indent=2) + "\n")
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


def publish(args, repo, expected_preparation=None):
    release = release_for(repo, args.tag)
    if release is None:
        raise ValueError("Run prepare first; no draft exists for this tag.")
    already_published = not release["draft"] and expected_preparation is not None
    if not already_published:
        owned_draft(release, args.commit)
    elif release["target_commitish"] != args.commit or MARKER not in (release["body"] or ""):
        raise ValueError("Refusing to accept an unrelated published release")
    with tempfile.TemporaryDirectory() as temp:
        downloaded = Path(temp) / "release"
        run("gh", "release", "download", args.tag, "--repo", repo, "--dir", downloaded)
        provenance = verify_assets(downloaded, args.tag, args.commit)
        if provenance["repository"] != repo:
            raise ValueError("Draft was prepared for a different repository.")
        run_id = int(provenance["workflow_run_id"])
        attempt = int(provenance["workflow_run_attempt"])
        if expected_preparation is not None and (
                (run_id, attempt) != expected_preparation or provenance.get("trigger") != "workflow_run"):
            raise ValueError("Draft does not belong to this automatic preparation attempt")
        build = gh(f"repos/{repo}/actions/runs/{run_id}/attempts/{attempt}")
        if (build["conclusion"] != "success" or build["event"] not in ("workflow_dispatch", "workflow_run")
                or build["path"] != WORKFLOW or build["head_branch"] != "main"
                or build["head_sha"] != provenance["workflow_commit"]
                or build["head_repository"]["full_name"] != repo
                or provenance.get("trigger", "workflow_dispatch") != build["event"]):
            raise ValueError("Draft needs a successful prepare run of the release workflow on main.")
        if build["event"] == "workflow_run":
            ci = gh(f"repos/{repo}/actions/runs/{int(provenance['ci_run_id'])}")
            if (ci["conclusion"] != "success" or ci["event"] != "push" or ci["head_branch"] != "main"
                    or ci["path"] != ".github/workflows/ci.yml" or ci["head_sha"] != args.commit
                    or ci["head_repository"]["full_name"] != repo):
                raise ValueError("Automatic publication needs successful CI for the exact source commit")
        original = Path(temp) / "original"
        run("gh", "run", "download", str(run_id), "--repo", repo,
            "--name", f"community-{args.tag}-{attempt}", "--dir", original)
        verify_assets(original, args.tag, args.commit)
        if any(digest(path) != digest(original / path.name) for path in downloaded.iterdir()):
            raise ValueError("Draft assets differ from the retained prepare-run artifact.")
        if remote_commit(repo, args.tag) != args.commit:
            raise ValueError("Tag moved after preparation.")
        if already_published:
            verify_uploaded(release, downloaded)
            print(f"Release already published and verified: {release['html_url']}")
            return
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
    parser.add_argument("operation", choices=["validate", "package", "prepare", "publish",
                                             "auto-plan", "create-tag", "auto-publish-plan", "auto-publish"])
    parser.add_argument("--tag")
    parser.add_argument("--commit")
    parser.add_argument("--directory", type=Path, default=ROOT / "target/release-assets")
    parser.add_argument("--installer", type=Path)
    parser.add_argument("--deb", type=Path)
    parser.add_argument("--rpm", type=Path)
    parser.add_argument("--macos-update", type=Path)
    parser.add_argument("--macos-signature", type=Path)
    parser.add_argument("--deb-signature", type=Path)
    parser.add_argument("--rpm-signature", type=Path)
    args = parser.parse_args()
    automatic = args.operation in ("auto-plan", "auto-publish-plan", "auto-publish")
    if not automatic:
        if not args.tag:
            parser.error("This operation requires --tag")
        version_for(args.tag)
    if args.operation == "package":
        if not all((args.commit, args.installer, args.deb, args.rpm, args.macos_update,
                    args.macos_signature, args.deb_signature, args.rpm_signature)):
            parser.error("package requires the commit, all installers, the macOS update, and all signatures")
        package(args)
        return
    repo = os.environ["GITHUB_REPOSITORY"]
    if os.environ.get("GITHUB_REF") != "refs/heads/main":
        raise ValueError("Run the release workflow from main.")
    if automatic:
        if os.environ.get("GITHUB_EVENT_NAME") != "workflow_run":
            raise ValueError("Automatic operations require a workflow_run event")
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        if args.operation == "auto-plan":
            automatic_plan(repo, event)
        elif args.operation == "auto-publish-plan":
            preparation = automatic_preparation(repo, event)
            if preparation is None:
                outputs(ready="false")
            else:
                outputs(ready="true", tag=preparation[0])
        else:
            automatic_publish(repo, event)
        return
    if args.operation == "validate":
        version, commit = validate_source(args.tag)
        if remote_commit(repo, args.tag) != commit:
            raise ValueError("Tag moved after checkout.")
        ci_run = successful_ci(repo, commit)
        existing = release_for(repo, args.tag)
        if existing:
            owned_draft(existing, commit)
        outputs(ready="true", automatic="false", operation=os.environ.get("RELEASE_OPERATION", "prepare"),
                tag=args.tag, version=version, commit=commit, ci_run=ci_run)
    elif not args.commit or not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        parser.error(f"{args.operation} requires a full validated --commit")
    elif args.operation == "create-tag":
        create_tag(repo, args.tag, args.commit)
    elif args.operation == "prepare":
        prepare(args, repo)
    else:
        publish(args, repo)


if __name__ == "__main__":
    main()
