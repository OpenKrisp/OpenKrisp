# On-demand self-hosted CI

GitHub schedules these jobs, receives their full logs and results, and displays the normal checks on each PR. The registered runners are scoped to `saeta-eth/OpenKrisp`. PR jobs accept branches from this repository; fork PRs do not execute on the host.

The macOS runner uses the logged-in desktop session for packaged WebView/window smoke tests. Its checkout and credentials are under the ignored `target/self-hosted/osx-arm64/` directory, separate from the working checkout. The Linux runner runs inside a dedicated Ubuntu 24.04 ARM64 Lima VM named `openkrisp-ci`, with 8 CPUs, 16 GiB RAM and a sparse 160 GiB disk. It shares no host directories or SSH agent. Docker and x86 binary emulation are installed inside the VM for container actions, including the pinned license action.

## Start

From the repository, run one command in each terminal:

```sh
python3 scripts/self-hosted-runner.py start
python3 scripts/self-hosted-linux.py start
```

The helpers verify official runner archives against the GitHub API checksum and register a runner only when needed. The Linux helper passes a short-lived registration token over stdin; the host's `gh` authentication remains on the Mac. No login service or automatic startup is installed. The Mac needs `gh`, Python 3.12+, Xcode command-line tools, Rust and Node. The Linux helper also needs Lima 2.0+; its VM installs its own build tools.

## Stop and remove

Ctrl-C in each terminal stops the corresponding runner. Queued jobs wait while a runner is offline. Stop the Linux VM when finished:

```sh
python3 scripts/self-hosted-linux.py stop
```

To unregister the Mac runner after stopping it:

```sh
python3 scripts/self-hosted-runner.py remove
```

Remove the Linux runner through the repository's Actions runner settings before deleting its VM with `limactl delete openkrisp-ci`. Stop and unregister a runner before deleting its credential directory. Leave the directories intact between sessions to reuse registration and build caches.

## Platform scope

PR and nightly jobs use real macOS ARM64 and Linux ARM64 runners labelled `openkrisp`. They retain the same tests and packaging checks. Hosted-runner image provisioning is replaced by the VM prerequisites and workflow dependency steps. Reference performance remains a separate calibrated job with its existing labels. Release Linux packages target x86-64 and still require an x86-64 release runner; the ARM64 PR result does not certify an x86-64 release build.

Self-hosted compute is currently free in GitHub Actions. GitHub artifact and cache storage have their own quotas and billing rules; job logs and pass/fail reporting do not depend on our custom commit-status publisher. Keep the repository's spending budget at zero if paid storage is unwanted.
