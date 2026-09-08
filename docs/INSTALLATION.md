# Installers and audio dependencies

OpenKrisp's virtual microphone requires BlackHole 2ch on macOS or a running PipeWire
desktop session on Linux. Dependency installation belongs to the OS installer/package
manager. The app checks the available audio endpoints when it starts; Linux creates
its virtual endpoints when a route starts.

## macOS

Use the combined OpenKrisp `.pkg`. It installs the app in `/Applications` and the standard
BlackHole 2ch driver in `/Library/Audio/Plug-Ins/HAL`. There is no Homebrew prerequisite or
download at install time. The installer requests administrator authentication and a restart
when installing BlackHole. It reuses the same or a newer installed BlackHole version.

After restarting, open OpenKrisp, allow microphone access, select a physical microphone,
and start Virtual microphone. Select **BlackHole 2ch** in your calling app. If the driver is
missing, rerun the combined installer. Deleting OpenKrisp leaves the shared driver installed.

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
It repacks the official BlackHole component without changing its driver payload or scripts.
Matching upstream source, build scripts, provenance, and GPL license are installed under
`/Library/Application Support/OpenKrisp/ThirdParty/BlackHole`.

For release builds, first sign and notarize the `.app` using Tauri's
[macOS signing procedure](https://v2.tauri.app/distribute/sign/macos/), then replace
`--development` with `--installer-identity 'Developer ID Installer: …'` and use the release
app path. The builder verifies app and upstream installer trust. Notarize the resulting
combined package with `xcrun notarytool submit ... --wait --keychain-profile ...`, then
`xcrun stapler staple ...`. Unsigned development installers are for local validation only.

**Distribution remains pending a project licensing decision.** BlackHole's upstream
[integration terms](https://github.com/ExistentialAudio/BlackHole#can-i-integrate-blackhole-into-my-app)
require GPLv3 for the integrating app or a separate license. OpenKrisp currently has no
project license; preparing the installer does not change that. Resolve this before publishing
the combined installer. Full OpenKrisp source/license release obligations must also be handled
if GPLv3 is selected; the bundled BlackHole source only covers that component.

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
services are shared with other apps and remain running.

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

The macOS development package was built and unpacked locally. macOS Installer's read-only
`-showChoicesXML` check selected both the app and missing driver on the development host.
The official BlackHole package
matched the pinned hash and passed macOS signature/notarization verification. Debian 12 and
Fedora 44 audio dependency installation passed in isolated containers. CI is configured to
build and inspect both Linux package formats; full desktop package installation is still
pending. No installer has been run on the
development Mac. Clean OS installation, upgrade/reinstall, GUI restart behavior, and real
microphone-to-calling-app acceptance remain required before calling this production-ready.
See the [hardware acceptance procedure](PHASE_5_HARDWARE_TEST.md).

Package names and upstream behavior were checked against the
[Debian PipeWire packages](https://packages.debian.org/bookworm/pipewire-audio),
[Fedora PipeWire packages](https://packages.fedoraproject.org/pkgs/pipewire/pipewire/),
[Fedora PulseAudio compatibility package](https://packages.fedoraproject.org/pkgs/pipewire/pipewire-pulseaudio/),
and [BlackHole installer source](https://github.com/ExistentialAudio/BlackHole/blob/master/Installer/create_installer.sh).
