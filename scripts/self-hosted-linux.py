#!/usr/bin/env python3
"""Manage the isolated Lima Linux runner using host GitHub authentication."""

import argparse
import json
from pathlib import Path
import shlex
import subprocess

ROOT = Path(__file__).resolve().parents[1]
VM = "openkrisp-ci"
REPOSITORY = "saeta-eth/OpenKrisp"


def github(endpoint, method="GET"):
    return json.loads(subprocess.check_output(["gh", "api", "--method", method, endpoint], text=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["prepare", "start", "stop"])
    args = parser.parse_args()
    if args.operation == "stop":
        subprocess.run(["limactl", "stop", VM], check=True)
        return
    instances = [json.loads(line) for line in subprocess.check_output(
        ["limactl", "list", "--json"], text=True
    ).splitlines() if line.strip()]
    instance = next((item for item in instances if item["name"] == VM), None)
    if instance is None:
        subprocess.run(["limactl", "start", "--yes", f"--name={VM}",
                        str(ROOT / ".github/runners/linux.yaml")], check=True)
    elif instance["status"] != "Running":
        subprocess.run(["limactl", "start", "--yes", VM], check=True)
    runner_home = subprocess.check_output(
        ["limactl", "shell", "--workdir=/tmp", VM, "printenv", "HOME"], text=True
    ).strip() + "/actions-runner"
    subprocess.run(["limactl", "copy", str(ROOT / "scripts/self-hosted-runner.py"),
                    f"{VM}:/tmp/openkrisp-self-hosted-runner.py"], check=True)
    registered = subprocess.run(["limactl", "shell", "--workdir=/tmp", VM,
                                 "test", "-f", runner_home + "/.runner"]).returncode == 0
    command = ["limactl", "shell", "--workdir=/tmp", VM, "python3",
               "/tmp/openkrisp-self-hosted-runner.py", "prepare", "--directory", runner_home]
    if registered:
        subprocess.run(command, check=True)
    else:
        packages = github(f"repos/{REPOSITORY}/actions/runners/downloads")
        package = next(item for item in packages if item["os"] == "linux" and item["architecture"] == "arm64")
        token = github(f"repos/{REPOSITORY}/actions/runners/registration-token", "POST")["token"]
        # Only the short-lived registration token crosses stdin; gh credentials stay on the host.
        subprocess.run(command + ["--bootstrap-stdin"],
                       input=json.dumps({"package": package, "token": token}), text=True, check=True)
    if args.operation == "start":
        subprocess.run(["limactl", "shell", "--workdir=" + runner_home, VM,
                        "sg", "docker", "-c", shlex.join([
                            "python3", "/tmp/openkrisp-self-hosted-runner.py", "start",
                            "--directory", runner_home])], check=True)


if __name__ == "__main__":
    main()
