# Windows support removed

Decision recorded 2026-09-08: **OpenKrisp targets macOS and Linux only**. Windows support has
been removed. This supersedes the earlier decision to retain it behind a development feature.

The removal includes:

- The AudioMirror router, feature flags and Windows-specific routing tests.
- WASAPI device-topology metadata and OpenKrisp's direct Windows API dependency.
- Windows UI states, frontend build settings and control-plane test accommodations.
- Windows application manifest, linker integration, packaging icons and capability target.
- The Windows CI job and virtual-cable candidate evaluation tools.

There is no development switch to restore the old backend. Desktop and audio I/O builds reject
platforms other than macOS and Linux. BlackHole and PipeWire remain the virtual microphone
integrations; their hardware, receiving-app and long-run acceptance is still required.

Cross-platform dependencies such as CPAL and Tauri can still list Windows packages in Cargo.lock.
Those transitive lockfile entries do not provide OpenKrisp Windows support.

## Decision evidence

AudioMirror was unfinished and had no validated production distribution for OpenKrisp. The
[Virtual Cables evaluation](VIRTUAL_CABLES_EVALUATION.md) found sample reordering, ignored device
mute and incomplete session shutdown. Its [recorded results](evidence/virtual-cables-2026-09-08.json)
remain as historical evidence. They are not an active integration or a future release commitment.

## Removal verification

- 148 Rust workspace tests and 19 frontend tests pass; one existing performance test is ignored.
- Frontend production build, formatting, warnings-denied Clippy and dependency-license checks pass.
- The Linux audio/router tests and live PipeWire stereo transfer/teardown test pass in the isolated
  Debian container, along with audio/router/engine Clippy.
- An attempted Windows audio I/O build is rejected with the explicit macOS/Linux-only diagnostic.
