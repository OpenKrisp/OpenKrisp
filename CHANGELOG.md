# Changelog

Automatically synchronized from published GitHub releases, newest first.
Draft releases are excluded; prereleases are included.

## [v0.1.0-beta.2](https://github.com/saeta-eth/OpenKrisp/releases/tag/v0.1.0-beta.2) — 2026-10-02 (prerelease)

# OpenKrisp 0.1.0 beta 2 — macOS and Linux

OpenKrisp reduces background noise in microphone and incoming call audio locally
on your computer. This experimental beta targets Apple Silicon Macs and x86-64
Linux desktops with PipeWire.

## Changes since beta 1

- Separate **Your microphone** and **Your speakers** controls simplify device
  selection and noise reduction setup.
- Audio startup stays visibly pending and can be cancelled. Audio level meters,
  saved preferences, and the app and tray icons have been improved.
- The release workflow now includes Linux DEB and RPM packages alongside the
  combined macOS app and virtual audio driver installer.
- Installer and dependency notice verification has been strengthened. Version
  releases are built, checked, and published through GitHub Actions.

## Download and install

Choose the package for your computer:

| Platform | Package | Target |
| --- | --- | --- |
| macOS | Apple Silicon `.pkg` | M-series Macs; macOS 11.0 minimum, full OS range not qualified |
| Debian / Ubuntu | x86-64 `.deb` | Debian 12 and Ubuntu 24.04 with an active PipeWire session |
| Fedora | x86-64 `.rpm` | Fedora 44 with an active PipeWire session |

The repository remains private. Downloads require repository read access.
macOS Intel, Linux ARM, Windows, and other Linux distributions are not qualified.
See the included `INSTALL-macos.md` or `INSTALL-linux.md` for installation,
routing, updates, and removal.

**All packages are unsigned, and the macOS build is not notarized by Apple.**
The macOS app and audio drivers use ad-hoc signatures. macOS may block the
installer or first launch; follow the installation guide for Apple's per-app
exception flow. Managed Macs may disallow it. Linux packages should be installed
through APT or DNF so their declared dependencies are installed too.

After installation, choose your physical microphone and headphones in OpenKrisp.
Turn on the relevant controls, then choose **OpenKrisp Microphone** as input and
**OpenKrisp Speaker** as output in your call app. On Linux, these endpoints appear
when the corresponding routes start. Headphones are recommended for this beta.

## Validation and beta limitations

The included `VALIDATION.md` records the automated checks for these exact release
files. Publication requires successful CI, both platform builds, installer
verification, and package installation checks on Debian 12, Ubuntu 24.04, and
Fedora 44. Those Linux installation checks run in containers.

No new real-machine acceptance results are claimed for beta 2. Fresh-Mac browser
download and Gatekeeper behavior, system installation, driver upgrades and
restart, and microphone permission still require manual acceptance. Linux
desktop launch and PipeWire routing, real calls on both platforms, speakerphone
echo cancellation, device changes, and long-running audio sessions are not fully
qualified. Automated checks do not establish those results.

Updates are manual: stop audio processing and quit OpenKrisp and your call apps
before installing a newer package. Removing the macOS app alone does not remove
its audio drivers; follow the installation guide.

OpenKrisp's original code is source available under PolyForm Perimeter 1.0.0.
Third-party components retain their own licenses. This release includes an exact
source snapshot, dependency notices, installation guides, and SHA-256 checksums.
Checksums detect changed downloads; they do not replace a publisher signature.


Source commit: `e3beabac6f0f51a35fb820cce739257f96940528`.

<!-- openkrisp-actions-release-v2 -->
