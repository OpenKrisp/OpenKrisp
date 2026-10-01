# Community beta distribution

The release workflow targets Apple Silicon Macs and x86-64 Linux (DEB and RPM).
The earlier manually prepared `0.1.0-beta.1` draft contains only macOS; its assets
and validation record are historical. The macOS package uses an optimized release
build, an ad-hoc signed app and audio drivers, and an unsigned `.pkg` installer.
It is **not notarized by Apple**. The build sets macOS 11.0 as its minimum version;
the complete macOS version and device matrix has not been qualified.

The maintainer confirmed written BlackHole permission for the current integration
on 2026-09-25; see [licensing status](LICENSING.md). Keep third-party notices and
source materials with the release. This permission record is separate from Apple
signing and does not mean Apple has reviewed the application.

## Release location and access

Decision recorded on 2026-09-25: keep `saeta-eth/OpenKrisp` private for now and
publish all releases in [this repository](https://github.com/saeta-eth/OpenKrisp/releases).
There is no separate downloads repository. Published private prereleases are
available to people with repository read access; sharing the download URL alone
does not grant access. Draft releases are preparation artifacts, not the tester
download channel. See [GitHub's release access documentation](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).

A future public launch will use this same repository and release history. Follow
the [public-launch preparation](PUBLIC_LAUNCH.md) before changing visibility.
Making the repository public does not change the PolyForm Perimeter license.

Use the [GitHub Actions release workflow](RELEASING.md) to prepare and publish
versions. Both operations require an explicit manual run. The local build steps
below are for development and troubleshooting; they do not publish a release.

## Installing on Linux

The workflow builds DEB on Debian 12 and RPM on Fedora 44, with package installation
checks on Debian 12, Ubuntu 24.04, and Fedora 44. Follow the
[Linux installation guide](../packaging/linux/INSTALL.md) for APT/DNF installation,
PipeWire session setup, routing, updates, and removal. These packages include the
model and third-party notices. Linux ARM and other distributions are not qualified.
The native release build/install jobs are configured but have not yet run. Hosted
routine CI resumed on 2026-10-01; it does not execute the manual Release workflow.

## Installing on macOS

1. Download the Apple Silicon `.pkg` from the project's release page. The package
   includes the app and both OpenKrisp virtual audio drivers.
2. Open the installer. If macOS blocks it because the developer cannot be verified,
   use **System Settings → Privacy & Security → Open Anyway** only after checking
   that the download came from the intended release page. Managed Macs may disallow
   this exception. See [Apple's instructions](https://support.apple.com/en-us/102445).
3. Close audio applications, authorize installation, and restart if prompted.
4. Open OpenKrisp from Applications and allow microphone access. The app may also
   require a separate Open Anyway exception on first launch.
5. In your call app, choose **OpenKrisp Microphone** as input and **OpenKrisp Speaker**
   as output. In OpenKrisp, choose your physical microphone and headphones and start
   the matching routes. See [installation and routing](INSTALLATION.md).

If the package or app reports that it is damaged or contains malware, stop and
report the exact message rather than disabling system security. This guide does
not require turning off Gatekeeper, SIP, or global security protections.

Updates are manual: quit OpenKrisp and your call apps, download the next installer,
and run it. The installer preserves newer driver versions and reinstalls missing
drivers. It may request a restart when a driver is installed or updated.

To uninstall, stop the routes and quit audio apps. Move OpenKrisp from Applications
to Trash. In Finder, use Go → Go to Folder to open `/Library/Audio/Plug-Ins/HAL`,
and move only `OpenKrispMicrophone.driver` and `OpenKrispSpeaker.driver` to Trash,
authorizing the change when prompted. Restart your Mac. Removing the app alone
does not remove these drivers. Leave any other audio drivers in place.

## Building on an Apple Silicon Mac

Install the pinned Rust, Node, Python, Meson, and Ninja prerequisites described in
the repository README. Build tools and registry dependencies must already be
available for the offline Cargo commands. Install frontend dependencies with
`npm ci --prefix apps/desktop` when preparing a fresh checkout.

Prepare third-party notices. The optional fetch retrieves missing license files
from the dependency versions' pinned upstream Git revisions, with cached hashes:

```sh
python3 scripts/prepare-release-notices.py --target aarch64-apple-darwin --fetch-missing
```

The collector refuses to overwrite `target/release-notices`; move an earlier
inventory aside before preparing a different release. It includes corresponding
source archives for MPL-2.0 crates and does not apply PolyForm terms to dependencies.

Build using the community configuration; it explicitly selects ad-hoc signing:

```sh
npm run --prefix apps/desktop tauri -- build --ci \
  --config src-tauri/tauri.community.conf.json -- --locked --offline
python3 scripts/build-macos-installer.py --community \
  --app target/release/bundle/macos/OpenKrisp.app \
  --output target/release/bundle/pkg/OpenKrisp-community-apple-silicon.pkg
python3 scripts/verify-installers.py target/release/bundle/pkg/OpenKrisp-community-apple-silicon.pkg
```

The community installer builder verifies the app's code signature and signs its
drivers ad hoc. A development build remains a separate mode. Developer ID signing
and notarization use the existing signed release mode when credentials become
available. Tauri explains the limitations of [ad-hoc signing](https://v2.tauri.app/distribute/sign/macos/#ad-hoc-signing).

## Release contents and validation

Publish a GitHub **prerelease**, with the platform and lack of notarization visible
in the release notes. Include the installer, a source snapshot of the exact release
commit, an installation guide, and SHA-256 checksums. A checksum detects a changed
download; it is not an Apple signature or an independent guarantee of origin.

Record the source commit and actual validation results. Complete the frontend,
Rust, dependency-license, installer, and native audio software checks before
uploading. Inspect the final package and all source-snapshot contents. Do not
publish private correspondence or local configuration files with the source.

Clean-Mac browser-download/Gatekeeper behavior, installed driver enumeration,
upgrade/restart, real-call listening, device transitions, and long-running audio
acceptance remain manual checks. Report those that have not been performed as
known beta limitations. Software tests and isolated driver probes do not prove
installed-device acceptance. The current Mac host cannot qualify Linux desktop
audio. Linux container installation checks are separate from GUI, routing,
real-call, device-transition, and long-running hardware acceptance. macOS Intel
and Linux ARM need their own future build and validation work.

GitHub Actions for the private source repository was blocked by account billing
on 2026-09-25 before any jobs started. Local checks must be recorded accurately;
do not describe that CI run as passing or alter billing settings as part of a release.

## Version publication

Follow [Releasing](RELEASING.md): merge a version PR, wait for main CI, tag that
commit, run **prepare**, review the draft, then explicitly run **publish**. Publish
uses the files from the successful preparation attempt and verifies them against
its retained Actions artifact. Do not bypass the workflow with a manual upload or
Publish button, since that skips its checks. No release is scheduled by these docs.
