#!/usr/bin/env python3
"""Prepare, register and run a repository-scoped GitHub Actions runner on demand."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "saeta-eth/OpenKrisp"


def github(endpoint, method="GET"):
    return json.loads(subprocess.check_output(
        ["gh", "api", "--method", method, endpoint], text=True
    ))


def runner_platform():
    systems = {"Darwin": "osx", "Linux": "linux"}
    architectures = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x64"}
    try:
        return systems[platform.system()], architectures[platform.machine()]
    except KeyError as error:
        raise SystemExit("This helper supports macOS/Linux on ARM64 or x64.") from error


def configure(directory, name, labels, remove=False):
    kind = "remove-token" if remove else "registration-token"
    token = github(f"repos/{REPOSITORY}/actions/runners/{kind}", "POST")["token"]
    environment = dict(os.environ, ACTIONS_RUNNER_INPUT_TOKEN=token)
    command = [str(directory / "config.sh")]
    if remove:
        command += ["remove", "--unattended"]
    else:
        command += ["--unattended", "--url", f"https://github.com/{REPOSITORY}",
                    "--name", name, "--labels", labels, "--work", "_work"]
    subprocess.run(command, cwd=directory, env=environment, check=True)


def prepare(directory, name, labels):
    if (directory / ".runner").exists():
        metadata = json.loads((directory / ".runner").read_text())
        if metadata["agentName"] != name or metadata["gitHubUrl"].rstrip("/") != f"https://github.com/{REPOSITORY}":
            raise SystemExit("Existing runner configuration belongs to another runner/repository.")
        print(f"Runner already registered: {name}")
        return
    if not (directory / "config.sh").exists():
        system, architecture = runner_platform()
        packages = github(f"repos/{REPOSITORY}/actions/runners/downloads")
        package = next(item for item in packages
                       if item["os"] == system and item["architecture"] == architecture)
        url = package["download_url"]
        if not url.startswith("https://github.com/actions/runner/releases/download/"):
            raise SystemExit("Unexpected runner download origin.")
        directory.mkdir(parents=True, exist_ok=True)
        archive = directory / package["filename"]
        print(f"Downloading official runner: {package['filename']}", flush=True)
        digest = hashlib.sha256()
        with urllib.request.urlopen(url, timeout=60) as response, archive.open("wb") as output:
            while block := response.read(1024 * 1024):
                digest.update(block)
                output.write(block)
        if digest.hexdigest() != package["sha256_checksum"]:
            archive.unlink()
            raise SystemExit("Runner archive checksum mismatch.")
        with tarfile.open(archive) as bundle:
            bundle.extractall(directory, filter="data")
        archive.unlink()
    configure(directory, name, labels)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["prepare", "start", "remove"])
    system, architecture = runner_platform()
    parser.add_argument("--directory", type=Path,
                        default=ROOT / "target" / "self-hosted" / f"{system}-{architecture}")
    parser.add_argument("--name", default=f"openkrisp-{system}-{architecture}")
    parser.add_argument("--labels", default="openkrisp")
    args = parser.parse_args()
    directory = args.directory.resolve()
    os.umask(0o077)
    if args.operation == "remove":
        if not (directory / ".runner").exists():
            raise SystemExit("Runner is not registered in this directory.")
        configure(directory, args.name, args.labels, remove=True)
        return
    prepare(directory, args.name, args.labels)
    if args.operation == "start":
        # No launch agent/service is installed. Ctrl-C stops accepting jobs.
        os.execv(str(directory / "run.sh"), [str(directory / "run.sh")])


if __name__ == "__main__":
    main()
