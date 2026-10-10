#!/usr/bin/env python3
"""Prepare, register and run a repository-scoped GitHub Actions runner on demand."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import signal
import subprocess
import sys
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


def configure(directory, name, labels, remove=False, token=None):
    kind = "remove-token" if remove else "registration-token"
    if token is None:
        token = github(f"repos/{REPOSITORY}/actions/runners/{kind}", "POST")["token"]
    environment = dict(os.environ, ACTIONS_RUNNER_INPUT_TOKEN=token)
    command = [str(directory / "config.sh")]
    if remove:
        command += ["remove", "--unattended"]
    else:
        command += ["--unattended", "--url", f"https://github.com/{REPOSITORY}",
                    "--name", name, "--labels", labels, "--work", "_work"]
    subprocess.run(command, cwd=directory, env=environment, check=True)


def install_hooks(directory):
    if platform.system() != "Linux":
        return
    # Container jobs create root-owned checkouts. Repair only this runner's workspace,
    # before the next checkout and after each job, without following symlinks.
    hook = directory / "hooks" / "workspace-owner.sh"
    hook.parent.mkdir(exist_ok=True)
    work = shlex.quote(str(directory / "_work"))
    hook.write_text(
        "#!/bin/bash\nset -euo pipefail\n" + f"runner_work={work}\n" +
        'if test -z "${GITHUB_WORKSPACE:-}"; then exit 0; fi\n'
        'workspace=$(realpath -m -- "$GITHUB_WORKSPACE")\n'
        'case "$workspace" in\n'
        '  "$runner_work/"*)\n'
        '    if test -d "$workspace"; then\n'
        '      sudo chown -R --no-dereference "$(id -u):$(id -g)" "$workspace"\n'
        '    fi\n    ;;\nesac\n'
    )
    hook.chmod(0o700)
    env_file = directory / ".env"
    keys = ("ACTIONS_RUNNER_HOOK_JOB_STARTED", "ACTIONS_RUNNER_HOOK_JOB_COMPLETED")
    lines = env_file.read_text().splitlines() if env_file.exists() else []
    lines = [line for line in lines if not any(line.startswith(key + "=") for key in keys)]
    env_file.write_text("\n".join(lines + [f"{key}={hook}" for key in keys]) + "\n")


def prepare(directory, name, labels, bootstrap=None):
    if (directory / ".runner").exists():
        metadata = json.loads((directory / ".runner").read_text(encoding="utf-8-sig"))
        if metadata["agentName"] != name or metadata["gitHubUrl"].rstrip("/") != f"https://github.com/{REPOSITORY}":
            raise SystemExit("Existing runner configuration belongs to another runner/repository.")
        print(f"Runner already registered: {name}")
        install_hooks(directory)
        return
    if not (directory / "config.sh").exists():
        system, architecture = runner_platform()
        packages = [bootstrap["package"]] if bootstrap else github(f"repos/{REPOSITORY}/actions/runners/downloads")
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
    configure(directory, name, labels, token=bootstrap["token"] if bootstrap else None)
    install_hooks(directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["prepare", "start", "stop", "remove"])
    system, architecture = runner_platform()
    parser.add_argument("--directory", type=Path,
                        default=ROOT / "target" / "self-hosted" / f"{system}-{architecture}")
    parser.add_argument("--name", default=f"openkrisp-{system}-{architecture}")
    parser.add_argument("--labels", default="openkrisp")
    parser.add_argument("--bootstrap-stdin", action="store_true",
                        help="Read official package metadata and a short-lived registration token from stdin.")
    args = parser.parse_args()
    directory = args.directory.resolve()
    os.umask(0o077)
    if args.operation == "stop":
        pattern = "^" + re.escape(str(directory / "bin/Runner.Listener")) + r" run( |$)"
        listeners = subprocess.run(["pgrep", "-f", pattern], capture_output=True, text=True)
        if listeners.returncode not in (0, 1):
            raise SystemExit("Could not inspect this runner's listener.")
        for pid in listeners.stdout.split():
            os.kill(int(pid), signal.SIGINT)
        return
    if args.operation == "remove":
        if not (directory / ".runner").exists():
            raise SystemExit("Runner is not registered in this directory.")
        configure(directory, args.name, args.labels, remove=True)
        return
    bootstrap = json.load(sys.stdin) if args.bootstrap_stdin else None
    prepare(directory, args.name, args.labels, bootstrap)
    if args.operation == "start":
        # No launch agent/service is installed. Ctrl-C stops accepting jobs.
        os.umask(0o022)
        if platform.system() == "Darwin":
            os.execvp("caffeinate", ["caffeinate", "-i", str(directory / "run.sh")])
        os.execv(str(directory / "run.sh"), [str(directory / "run.sh")])


if __name__ == "__main__":
    main()
