# OpenKrisp desktop

Original OpenKrisp code uses [PolyForm Perimeter 1.0.0](LICENSE). See the root
[NOTICE](../../NOTICE) for scope and [licensing status](../../docs/LICENSING.md)
for the unresolved BlackHole permission required before combined macOS distribution.

The Phase 4 desktop control plane uses Tauri v2, Vite, React, and strict TypeScript. It owns only
commands, low-rate events, snapshots, profiles, windows, and tray lifecycle; PCM remains entirely
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
deterministic engine double, crosses the bounded Rust event bridge, reopens profile storage, and
checks the production capability allowlist. `npm test --prefix apps/desktop` independently verifies
the TypeScript command envelopes, listener lifecycle, and snapshot reconciliation. Neither suite
opens an audio device or sends PCM across the desktop boundary.


OpenKrisp targets macOS and Linux only, using named BlackHole-based drivers and native PipeWire respectively.
The setup panel provides instructions for the selected platform.
Linux creates **OpenKrisp Microphone** on Start and removes endpoints owned by that route on
Stop/Quit. DEB/RPM packages require the complete PipeWire runtime, WirePlumber and PulseAudio
compatibility; install with APT/DNF so dependencies are fetched. Log out and back in after
first installation. ALSA-only/PulseAudio-only sessions support physical monitoring.
The combined macOS package includes **OpenKrisp Microphone** for the call microphone and
**OpenKrisp Speaker** for incoming call playback. Choose those devices in the calling app,
then start both OpenKrisp routes with a physical incoming output. See [installer builds and remaining
release requirements](../../docs/INSTALLATION.md).

Windows support, its development feature, packaging integration and CI job have been removed.
The desktop and audio I/O reject unsupported build targets. See [the removal decision](../../docs/WINDOWS_SUPPORT.md).

Linux builds also need `libpipewire-0.3-dev` (>= 0.3.53), `libasound2-dev`, `pkg-config`, and
`libclang-dev`, alongside the existing Tauri headers. Linux defaults to DEB/RPM; macOS builds
an app bundle used by `scripts/build-macos-installer.py`. Raw app/source builds require manual
audio dependency setup. AppImage/DMG are no longer default distribution artifacts.

The everyday screen gives **Your microphone** and **Your speakers** equal space
in two permanently visible panels. The default 1080 × 720 window shows them side
by side; narrower windows stack them. Each has an illustrated flow, independent
status, device picker, **Turn on OpenKrisp / Turn off OpenKrisp** action and the exact
device name to select in your call app. Every start enables noise reduction.
The illustrations follow actual processing state and show headphones in microphone
test mode. **Your speakers** shows your call app → OpenKrisp → headphones or speakers.
Choose where to listen and select **Turn on OpenKrisp** to enable speech noise reduction. A
separate connection panel shows the exact speaker name to select in the call app.
For music, use the call app’s usual headphones or speakers directly. Switch back
to those devices before selecting **Turn off OpenKrisp** in Your speakers.
**Help** contains matching microphone and speaker connection guides, device refresh,
and **Copy support details**. Latency, noise-reduction tuning, echo cancellation and
virtual-device routing use the built-in defaults. Their controls and live diagnostic
tables have been removed from the desktop. Existing physical device choices are
preserved; retired technical preferences and bypass reset for the
next start without changing active audio. Running audio can be stopped from either
view. Choices are saved automatically; reopening the app never
starts audio. See the [UX review and design decisions](../../docs/DESKTOP_UX_REDESIGN.md).
Physical microphone and conferencing-app acceptance is still required on each target OS.
