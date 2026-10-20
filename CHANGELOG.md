# Changelog

Automatically synchronized from published GitHub releases, newest first.
Draft releases are excluded; prereleases are included.

## [v0.1.0-beta.4](https://github.com/OpenKrisp/OpenKrisp/releases/tag/v0.1.0-beta.4) — 2026-10-20 (prerelease)

# OpenKrisp 0.1.0 beta 4 — fully open source under GPLv3

OpenKrisp reduces background noise in microphone and incoming call audio locally
on your computer. This experimental beta targets Apple Silicon Macs and x86-64
Linux desktops with PipeWire.

## Changes since beta 3

- OpenKrisp is now fully open source under **GNU GPLv3 only (`GPL-3.0-only`)**.
  This beta rebuilds the application and installers with the new license,
  matching the public source and the downloads on [openkrisp.com](https://openkrisp.com/).
- Source, releases, and the signed update feed are publicly accessible from
  `OpenKrisp/OpenKrisp`, without a repository invitation or embedded credentials.
- Cargo, npm, and Tauri license metadata, bundled license texts, the macOS
  installer license display, and contributor documentation use GPLv3. Third-party
  components retain their own licenses, notices, and required source materials.
- This is a licensing and distribution update. It introduces no audio-processing
  changes since beta 3; noise reduction continues to run locally on your device.

Earlier beta artifacts retain their original bundled terms. They have not been
replaced or relabeled. See [the licensing record](https://github.com/OpenKrisp/OpenKrisp/blob/v0.1.0-beta.4/docs/LICENSING.md).

## Download and install

| Platform | Package | Target |
| --- | --- | --- |
| macOS | Apple Silicon `.pkg` | M-series Macs; macOS 11.0 minimum, full OS range not qualified |
| Debian / Ubuntu | x86-64 `.deb` | Debian 12 and Ubuntu 24.04 with an active PipeWire session |
| Fedora | x86-64 `.rpm` | Fedora 44 with an active PipeWire session |

Downloads are public. macOS Intel, Linux ARM, Windows, and other Linux distributions
are not qualified. See the included `INSTALL-macos.md` or `INSTALL-linux.md` for
installation, routing, updates, and removal.

**Native installer packages are unsigned, and the macOS build is not notarized
by Apple.** The macOS app and audio drivers use ad-hoc signatures. Update artifacts
have separate cryptographic signatures verified by OpenKrisp; these do not replace
Apple notarization or native package signing. macOS may block the installer or
first launch; follow the installation guide for Apple's per-app exception flow.
Managed Macs may disallow it. Install Linux packages through APT or DNF so their
declared dependencies are installed too.

Beta 3 can check the public update feed through **Help → Check for updates**.
Application updates install when both audio routes are off, after a 30-second
countdown; choose **Later** to postpone. To upgrade manually, stop processing,
quit OpenKrisp and your call apps, and install the new package. Upgrades from beta 2
or changes to macOS drivers and the uninstaller require the combined `.pkg`.
Restart if prompted. Application-only updates preserve installed drivers and the
uninstaller. Signed upgrade and restart behavior still require real-machine
acceptance on each platform.

After installation, choose your physical microphone and headphones in OpenKrisp.
Turn on the relevant controls, then choose **OpenKrisp Microphone** as input and
**OpenKrisp Speaker** as output in your call app. On Linux, these endpoints appear
when the corresponding routes start. Headphones are recommended for this beta.

## Source and license

The release includes `OpenKrisp-0.1.0-beta.4-source.tar.gz`, an exact source snapshot
of the tagged commit with the build and installation scripts. Bundled notices
retain required third-party source archives and build inputs, including the
macOS virtual audio drivers. Keep these materials and license notices when
redistributing. See `LICENSE`, `NOTICE`, and `docs/LICENSING.md` in the source.

## Validation and beta limitations

The included `VALIDATION.md` records the automated checks for these exact release
files. Publication requires successful CI, native macOS and Linux builds, installer
verification, updater signature verification, and package installation checks on
Debian 12, Ubuntu 24.04, and Fedora 44. Linux installation checks run in containers.

No new real-machine acceptance results are claimed for beta 4. Fresh-Mac browser
download and Gatekeeper behavior, system installation, driver upgrades and restart,
microphone permission, and removal still require manual acceptance. Linux desktop
launch and PipeWire routing, real calls on both platforms, speakerphone echo
cancellation, device changes, and long-running audio sessions are not fully
qualified. Automated tests do not establish those results.

Automatic update installation and restart also need real-machine acceptance on
macOS, Debian/Ubuntu, and Fedora, including administrator cancellation and retry,
preserved settings, and beta-to-stable upgrades.

The release also includes installation guides, signed update assets, provenance,
and SHA-256 checksums. Checksums detect changed downloads; they do not replace a
publisher signature.


Source commit: `0523fbf06fcf692f2914c3b8f7b92434d195cd46`.

<!-- openkrisp-actions-release-v2 -->

## [v0.1.0-beta.3](https://github.com/OpenKrisp/OpenKrisp/releases/tag/v0.1.0-beta.3) — 2026-10-16 (prerelease)

# OpenKrisp 0.1.0 beta 3 — automatic updates and macOS removal

OpenKrisp reduces background noise in microphone and incoming call audio locally
on your computer. This experimental beta targets Apple Silicon Macs and x86-64
Linux desktops with PipeWire.

## Changes since beta 2

- Signed application updates can download in the background and install when
  both audio routes are off. A 30-second countdown gives you time to choose
  **Later**, and **Help → Check for updates** checks immediately. The update
  feed becomes available when this repository is public; private beta downloads
  still require a manual installation.
- The macOS installer now includes **Uninstall OpenKrisp**. Use it from
  Applications or **OpenKrisp → Uninstall OpenKrisp…** to remove the app, its two
  virtual audio drivers, and package receipts while preserving saved settings.
- Automated coverage now exercises the production audio graph, speech quality,
  concurrent routes, recovery, and packaged desktop startup and window lifecycle.
  Scheduled extended tests cover longer audio sessions and performance.
- Release and CI fixes improve clean builds, cached package regeneration,
  installer checks, updater signature verification, and changelog synchronization.
- The project now lives at `OpenKrisp/OpenKrisp` on GitHub. Update checks and
  installation/help links use the organization repository.

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

**Native installer packages are unsigned, and the macOS build is not notarized
by Apple.** The macOS app and audio drivers use ad-hoc signatures. Update
artifacts have separate cryptographic signatures verified by OpenKrisp; these
do not replace Apple notarization or native package signing. macOS may block
the installer or first launch; follow the installation guide for Apple's
per-app exception flow. Managed Macs may disallow it. Install Linux packages
through APT or DNF so their declared dependencies are installed too.

To upgrade from beta 2, stop audio processing and quit OpenKrisp and your call
apps, then install the new package manually. On macOS, use the combined `.pkg`
to install the new uninstaller along with the app and audio drivers. Restart
if prompted. Future macOS application updates preserve installed drivers and
the uninstaller; changes to those components still require the combined `.pkg`.

After installation, choose your physical microphone and headphones in OpenKrisp.
Turn on the relevant controls, then choose **OpenKrisp Microphone** as input and
**OpenKrisp Speaker** as output in your call app. On Linux, these endpoints appear
when the corresponding routes start. Headphones are recommended for this beta.

## Validation and beta limitations

The included `VALIDATION.md` records the automated checks for these exact release
files. Publication requires successful CI, both platform builds, installer
verification, updater signature verification, and package installation checks on
Debian 12, Ubuntu 24.04, and Fedora 44. Linux installation checks run in containers.

No new real-machine acceptance results are claimed for beta 3. Fresh-Mac browser
download and Gatekeeper behavior, system installation, driver upgrades and
restart, microphone permission, and removal still require manual acceptance.
Linux desktop launch and PipeWire routing, real calls on both platforms,
speakerphone echo cancellation, device changes, and long-running audio sessions
are not fully qualified. Automated tests do not establish those results.

Automatic update installation and restart also need real-machine acceptance on
macOS, Debian/Ubuntu, and Fedora, including administrator cancellation and retry,
preserved settings, and beta-to-stable upgrades. The private repository prevents
anonymous update checks and downloads; no repository credentials ship in the app.

OpenKrisp's original code is source available under PolyForm Perimeter 1.0.0.
Third-party components retain their own licenses. This release includes an exact
source snapshot, dependency notices, installation guides, signed update assets,
and SHA-256 checksums. Checksums detect changed downloads; they do not replace a
publisher signature.


Source commit: `6c388476a3ffe019e4c6e5d48f9bb476ef50ab8c`.

<!-- openkrisp-actions-release-v2 -->

## [v0.1.0-beta.2](https://github.com/OpenKrisp/OpenKrisp/releases/tag/v0.1.0-beta.2) — 2026-10-02 (prerelease)

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
