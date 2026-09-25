# Installers and audio dependencies

OpenKrisp's virtual devices are **OpenKrisp Microphone** for outgoing cleaned speech
and **OpenKrisp Speaker** for incoming call playback. macOS uses dedicated builds of
BlackHole; Linux uses a running PipeWire desktop session. Dependency installation
belongs to the OS installer/package manager. The app checks available endpoints at
startup; Linux creates its virtual endpoints when a route starts.

## macOS

Use the combined OpenKrisp `.pkg`. It installs the app in `/Applications` and
`OpenKrispMicrophone.driver` / `OpenKrispSpeaker.driver` in `/Library/Audio/Plug-Ins/HAL`.
There is no Homebrew prerequisite or download at install time. The installer requests
administrator authentication and a restart when installing either driver. It independently
reuses each same-or-newer installed version and repairs a missing driver or executable.
Existing official BlackHole drivers are preserved and are not relabeled or replaced.

After restarting, open OpenKrisp, allow microphone access, select a physical microphone,
and configure both directions:

| Setting | Device |
| --- | --- |
| OpenKrisp microphone input | Your physical microphone |
| OpenKrisp microphone destination | Virtual microphone → OpenKrisp Microphone |
| Calling app microphone | OpenKrisp Microphone |
| Calling app speaker | OpenKrisp Speaker, stereo channels 1–2 |
| OpenKrisp Incoming audio source | OpenKrisp Speaker |
| OpenKrisp Incoming audio output | Your physical headphones or speakers |

Start both OpenKrisp routes; either may start first. For speakers, check that microphone
AEC reports active. Keep general music in incoming bypass. OpenKrisp captures only
OpenKrisp Speaker channels 1–2, without mixing channels 3–16. This is stereo call support,
not surround playback. Do not select an aggregate/multi-output device or send either
OpenKrisp output back into the incoming bus.

Only the two named OpenKrisp devices are supported. Saved selections pointing at old
BlackHole endpoints cannot start: select the new devices in **both** OpenKrisp and your
call app. OpenKrisp saves settings automatically. Missing or unsupported selections never
silently fall back to another bus.

Raw `.app` builds do not install drivers; use the combined installer. Deleting the app
does not remove audio drivers. Stop both routes and quit audio apps before installing;
the installer requests a restart instead of killing the audio server.

The user's [macOS Google Meet check](GOOGLE_MEET_USER_TEST_2026-09-18.md) passed on the
previous setup. It does not validate these newly built drivers. Installation, enumeration
in a call app, upgrade/restart and listening checks for the named drivers remain pending.
Linux hardware/conferencing is untested. Broader AEC, transitions and two-hour criteria
also remain open. See [named-device implementation](VIRTUAL_DEVICE_NAMES.md).

Build a local test installer from the repository root:

```sh
npm ci --prefix apps/desktop
npm run --prefix apps/desktop tauri -- build --debug
python3 scripts/build-macos-installer.py --development \
  --app target/debug/bundle/macos/OpenKrisp.app \
  --output target/debug/bundle/pkg/OpenKrisp-dev.pkg
python3 scripts/verify-installers.py target/debug/bundle/pkg/OpenKrisp-dev.pkg
```

The builder refuses to overwrite an existing output. Build-time downloads are cached under
`target/installer-cache` and checked against committed SHA-256 hashes, including cache hits.
The builder compiles the pinned BlackHole 0.7.1 source into two universal ARM64/x86_64
HAL bundles using Apple's command-line tools. It customizes names, independent bundle/UID/
factory identities, channel counts and icon through build settings without modifying the
upstream C source. The microphone stays stereo; the speaker keeps the existing 16-channel
transport with only channels 1–2 captured. There are no postinstall scripts that modify
other drivers or restart CoreAudio. The original source, license, build script, branding
inputs and provenance are installed under
`/Library/Application Support/OpenKrisp/ThirdParty/BlackHole`.

For Developer ID releases, first sign and notarize the `.app` using Tauri's
[macOS signing procedure](https://v2.tauri.app/distribute/sign/macos/), then replace
`--development` with both `--installer-identity 'Developer ID Installer: …'` and
`--driver-identity 'Developer ID Application: …'`, using the release app path.
The builder verifies the app's trust and signs/verifies each custom driver. Notarize the
resulting combined package with `xcrun notarytool submit ... --wait --keychain-profile ...`,
then `xcrun stapler staple ...`. Official BlackHole signatures do not cover these custom
builds. Development drivers are ad-hoc signed and the outer installer is unsigned;
these artifacts are for local validation only.

For a community beta without an Apple Developer membership, use the explicit
ad-hoc signing configuration and `--community` installer mode described in the
[community release guide](COMMUNITY_RELEASE.md). These builds are not notarized
and macOS may require a per-app security exception. The maintainer confirmed
written permission for the current BlackHole integration on 2026-09-25; see
[licensing scope and status](LICENSING.md). Third-party licenses remain separate.

## Linux

The default Linux outputs are `.deb` and `.rpm`. Install through a package manager that
resolves dependencies:

```sh
# Debian / Ubuntu (replace the filename with the downloaded package)
sudo apt install ./OpenKrisp.deb

# Fedora (replace the filename with the downloaded package)
sudo dnf install ./OpenKrisp.rpm
```

| Component | Debian/Ubuntu dependency | Fedora dependency |
| --- | --- | --- |
| Native audio library | `libpipewire-0.3-0 (>= 0.3.53)` | `pipewire-libs >= 0.3.53` |
| Audio server | `pipewire (>= 0.3.53)` | `pipewire >= 0.3.53` |
| Virtual cable tool | `pipewire-bin` | `pipewire-utils` |
| Device/session management | `wireplumber` | `wireplumber` |
| Calling app compatibility | `pipewire-pulse` | `pipewire-pulseaudio` |

The two versioned RPM requirements use one RPM boolean expression because Tauri 2.11.4's
RPM bundler passes dependency strings through as names. Plain `name >= version` strings
would request a nonexistent literal package name.

Dependencies come from the distribution's repositories and need network access unless
already installed/cached. Existing installations are reused. `dpkg -i` and `rpm -i` alone do
not fetch missing dependencies. The package manager displays any proposed audio-stack
changes; a PulseAudio installation can require migration to PipeWire. OpenKrisp does not
force-remove conflicting packages or restart an active desktop audio session.

Log out and back in after the first installation, then open OpenKrisp. The distribution's
user services must start PipeWire, WirePlumber and PulseAudio compatibility. If services
were previously masked or disabled, repair that session configuration before launching
the app. Restart OpenKrisp after repairing a session: its audio backend is selected at launch.

Start Virtual microphone and choose **OpenKrisp Microphone** in the calling app. The route
owns only the endpoints it creates; Stop/Quit removes those endpoints. System PipeWire
services are shared with other apps and remain running. Start Incoming audio, then select
**OpenKrisp Speaker** as the call app speaker. Its internal PipeWire node IDs are unchanged.
Linux hardware/conferencing remains untested; this naming change does not establish acceptance.

Debian 12 and Ubuntu 24.04 provide the required PipeWire packages; older distributions
with PipeWire below 0.3.53 need an OS upgrade or compatible packages. RPM dependency names
target Fedora. Other distributions, including Arch, currently need a source build and their
equivalent system packages. AppImage is no longer a default output because it cannot install
these system services. Raw `.app`/AppImage/source builds require separate dependency setup.

Build and inspect Linux packages on a Linux host with the Tauri and PipeWire development headers:

```sh
npm run --prefix apps/desktop tauri -- build --debug
python3 scripts/verify-installers.py target/debug/bundle/deb/*.deb target/debug/bundle/rpm/*.rpm
```

## Verification and remaining acceptance

Automated checks cover corrupted download rejection, fresh/upgrade/repaired driver selection,
combined package contents and restart metadata, and generated DEB/RPM dependency metadata.
The Debian container test installs the audio dependencies from the actual Tauri configuration
using APT; the Fedora test does the same with RPM/DNF. Both check that all four runtime tools
are present:

```sh
python3 -m unittest discover -s scripts/tests -v
docker run --rm -v "$PWD:/workspace:ro" -w /workspace rust:1.98-slim-bookworm \
  bash scripts/test-debian-audio-dependencies.sh
docker run --rm -v "$PWD:/workspace:ro" -w /workspace fedora:44 \
  bash scripts/test-fedora-audio-dependencies.sh
```

The named-driver development package is checked by unpacking it and loading both HAL
plugins together in an isolated process. This verifies their reported names, UIDs, native
channel counts, virtual transport and hidden mirror state without installing them or
contacting the audio server. Both binary architectures, signatures, installer choices and
matching source/build inputs are also checked. This is not installed-device acceptance.

Historical Debian 12/Fedora 44 dependency tests passed in isolated containers. Linux
hardware and conferencing remain untested. Clean OS installation, upgrade/reinstall,
GUI restart behavior and call audio with the new drivers still require the
[hardware acceptance procedure](PHASE_5_HARDWARE_TEST.md).

Package names and upstream behavior were checked against the
[Debian PipeWire packages](https://packages.debian.org/bookworm/pipewire-audio),
[Fedora PipeWire packages](https://packages.fedoraproject.org/pkgs/pipewire/pipewire/),
[Fedora PulseAudio compatibility package](https://packages.fedoraproject.org/pkgs/pipewire/pipewire-pulseaudio/),
and [BlackHole installer source](https://github.com/ExistentialAudio/BlackHole/blob/master/Installer/create_installer.sh).
