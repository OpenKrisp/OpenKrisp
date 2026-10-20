# Community beta distribution

The release workflow targets Apple Silicon Macs and x86-64 Linux (DEB and RPM).
The earlier manually prepared `0.1.0-beta.1` draft contains only macOS; its assets
and validation record are historical. The macOS package uses an optimized release
build, an ad-hoc signed app and audio drivers, and an unsigned `.pkg` installer.
It is **not notarized by Apple**. The build sets macOS 11.0 as its minimum version;
the complete macOS version and device matrix has not been qualified.

Current OpenKrisp source uses GNU GPLv3 only, matching BlackHole's published GPLv3
integration route; see [licensing status](LICENSING.md). Keep third-party notices
and corresponding source/build materials with the release. The earlier separate
permission record concerns historical PolyForm releases and is unrelated to Apple
signing or application review.

## Release location and access

The repository moved to the OpenKrisp organization in October 2026 and became
public on 2026-10-20. Publish all releases in
[this repository](https://github.com/OpenKrisp/OpenKrisp/releases).
There is no separate downloads repository. Published releases and prereleases
are publicly downloadable. Draft releases remain preparation artifacts for
maintainers. See [GitHub's release access documentation](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).

The public source launch uses the existing repository and release history; see
the [launch record and remaining validation](PUBLIC_LAUNCH.md).
Current source is GPLv3-licensed. Making the repository public does not change
the bundled license of historical beta installers. Publish a new GPLv3 release
with matching corresponding source; preserve earlier assets and validation records.

Use the [GitHub Actions release workflow](RELEASING.md) to prepare and publish
versions. A version change merged into `main` is automatically built after CI
passes and published after the complete preparation run succeeds. Manual draft
preparation remains available for review before publication. The local build steps
below are for development and troubleshooting; they do not publish a release.

## Installing on Linux

The workflow builds DEB on Debian 12 and RPM on Fedora 44, with package installation
checks on Debian 12, Ubuntu 24.04, and Fedora 44. Follow the
[Linux installation guide](../packaging/linux/INSTALL.md) for APT/DNF installation,
PipeWire session setup, routing, updates, and removal. These packages include the
model and third-party notices. Linux ARM and other distributions are not qualified.
The native release build/install jobs are configured but have not yet run. Hosted
routine CI resumed on 2026-10-01; release builds run when a version merge passes CI.

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

Updater-enabled releases download application updates automatically from the
public release feed. When both routes are off, a 30-second countdown installs and restarts
automatically; select **Later** to postpone. See
[automatic updates](AUTOMATIC_UPDATES.md) for signing setup and acceptance checks.
For older builds or changes to audio drivers/uninstaller, quit OpenKrisp and your
call apps, download the next installer,
and run it. The installer preserves newer driver versions and reinstalls missing
drivers. It may request a restart when a driver is installed or updated.

To uninstall, choose **OpenKrisp → Uninstall OpenKrisp…**, or open
**Uninstall OpenKrisp** in Applications if you have already trashed OpenKrisp.
The installer includes this app; no additional download is needed. Close calling
and audio apps, confirm removal, and authorize it with your Mac's administrator
password. The uninstaller quits OpenKrisp and removes the application, its two
audio drivers, and package receipts. Restart afterward to unload the drivers.
Other audio drivers and saved settings are preserved. The uninstaller remains
in Applications for retrying cleanup; you can move it to Trash when finished.

For older installations without the uninstaller, open `/Library/Audio/Plug-Ins/HAL`
and move only `OpenKrispMicrophone.driver` and `OpenKrispSpeaker.driver` to Trash,
authorize the change, and restart.

## Building on an Apple Silicon Mac

Install the pinned Rust, Node, Python, Meson, and Ninja prerequisites described in
the [contributor guide](../CONTRIBUTING.md#set-up-a-development-environment).
Build tools and registry dependencies must already be available for the offline
Cargo commands. Install frontend dependencies with
`npm ci --prefix apps/desktop` when preparing a fresh checkout.

Prepare third-party notices. The optional fetch retrieves missing license files
from the dependency versions' pinned upstream Git revisions, with cached hashes:

```sh
python3 scripts/prepare-release-notices.py --target aarch64-apple-darwin --fetch-missing
```

The collector refuses to overwrite `target/release-notices`; move an earlier
inventory aside before preparing a different release. It includes corresponding
source archives for MPL-2.0 crates and preserves each dependency's own license terms.

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
commit, complete corresponding source/build inputs required by GPLv3, an
installation guide, and SHA-256 checksums. Link the source beside the binaries
and preserve third-party license/source materials. A checksum detects a changed
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

Follow [Releasing](RELEASING.md): merge a version PR with matching versions and
notes. Successful main CI starts tag creation and preparation; successful
preparation starts automatic publication. Complete any intended manual acceptance
before merging the version PR, since this path has no draft approval pause. Publish
uses the files from the successful preparation attempt and verifies them against
its retained Actions artifact. Do not bypass the workflow with a manual upload or
Publish button, since that skips its checks. Manual **prepare** and **publish**
operations remain available for troubleshooting and controlled draft review.
