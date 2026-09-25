# OpenKrisp desktop

Original OpenKrisp code uses [PolyForm Perimeter 1.0.0](LICENSE). See the root
[NOTICE](../../NOTICE) for scope and [licensing status](../../docs/LICENSING.md)
for the maintainer's confirmation of permission for the current BlackHole integration.

The desktop control plane uses Tauri v2, Vite, React, and strict TypeScript. It owns only
commands, low-rate events, snapshots, settings, windows, and tray lifecycle; PCM remains entirely
inside the Rust audio engine.

Useful commands:

```text
npm ci
npm run dev
npm run typecheck
npm test
npm run build
npm run tauri dev
npm run tauri build -- --debug --bundles app
```

Build the frontend before invoking workspace-wide Cargo checks directly; the default Tauri
`custom-protocol` feature embeds `dist` for normal and packaged binaries.

From the repository root, the focused Rust integration harness is:

```text
cargo test -p openkrisp-desktop --lib control_plane_harness
```

It submits real mock-runtime `InvokeRequest`s to the same handler used in production, uses a
deterministic engine double, crosses the bounded Rust event bridge, reopens settings storage, and
checks the production capability allowlist. `npm test --prefix apps/desktop` independently verifies
the TypeScript command envelopes, listener lifecycle, and snapshot reconciliation. Neither suite
opens an audio device or sends PCM across the desktop boundary.


OpenKrisp targets macOS and Linux only, using BlackHole and native PipeWire respectively.
The setup panel provides instructions for the selected platform.
Linux creates **OpenKrisp Microphone** on Start and removes endpoints owned by that route on
Stop/Quit. DEB/RPM packages require the complete PipeWire runtime, WirePlumber and PulseAudio
compatibility; install with APT/DNF so dependencies are fetched. Log out and back in after
first installation. ALSA-only/PulseAudio-only sessions support physical monitoring.
The combined macOS package includes dedicated BlackHole-based drivers named **OpenKrisp
Microphone** for the call microphone and **OpenKrisp Speaker** for incoming call playback
on channels 1–2. Choose those devices in the calling app,
then start both OpenKrisp routes with a physical incoming output. See [installer builds and remaining
release requirements](../../docs/INSTALLATION.md).

Windows support, its development feature, packaging integration and CI job have been removed.
The desktop and audio I/O reject unsupported build targets. See [the removal decision](../../docs/WINDOWS_SUPPORT.md).

Linux builds also need `libpipewire-0.3-dev` (>= 0.3.53), `libasound2-dev`, `pkg-config`, and
`libclang-dev`, alongside the existing Tauri headers. Linux defaults to DEB/RPM; macOS builds
an app bundle used by `scripts/build-macos-installer.py`. Raw app/source builds require manual
audio dependency setup. AppImage/DMG are no longer default distribution artifacts.

The desktop has route selectors, start/stop, bypass, peak meters, denoise parameter
editing, and diagnostics. Settings are saved automatically; there is no manual Profiles
panel. See [automatic settings](../../docs/AUTOMATIC_SETTINGS_PERSISTENCE.md).
Installed-device and conferencing-app acceptance is still required on each target OS.
Start with the root [contributor guide](../../CONTRIBUTING.md) for native build prerequisites.
