# OpenKrisp virtual device names

The call app and OpenKrisp now use the same role names: **OpenKrisp Microphone** for
outgoing processed speech and **OpenKrisp Speaker** for incoming playback. This change
builds on main `e2d045e` (PR #13); it does not alter the audio-engine DSP graph or AEC.

## macOS native names

The official BlackHole device name is a read-only CoreAudio property backed by a
compiled string. An app-only label would leave calling apps showing BlackHole.
The installer instead builds two dedicated drivers from the existing hash-pinned
BlackHole 0.7.1 source using upstream's
[customization constants](https://github.com/ExistentialAudio/BlackHole/tree/v0.7.1#customizing-blackhole).
The upstream C source is unchanged. Each driver has its own bundle, native UID,
model/box/mirror identities and CFPlugIn factory UUID, so it can coexist with the
other OpenKrisp driver and official BlackHole installations.

| Role | Call app name | Logical endpoint | Native identity | Native channels |
| --- | --- | --- | --- | --- |
| Microphone | OpenKrisp Microphone | `macos:openkrisp-microphone` | `coreaudio:OpenKrispMicrophone_UID` | 2 |
| Incoming audio | OpenKrisp Speaker | `macos:openkrisp-speaker` | `coreaudio:OpenKrispSpeaker_UID` | 16; capture 1–2 only |

The incoming driver retains the existing transport width and stereo selection; the
change does not introduce a new resampling/channel mapping path. Display names are
not trusted for discovery. Exact UID, CoreAudio host, virtual transport, input/output
roles and expected channel widths remain required. Native mirrors share ancestry with
their public device. Neither public nor mirror identities can become physical
microphone sources or incoming outputs, even if misclassified in inventory.

macOS routing supports only these two OpenKrisp endpoints. BlackHole 2ch/16ch
compatibility, legacy endpoint discovery and incoming capture through the microphone
bus have been removed. Unsupported IDs in existing profiles fail validation; users
select the new devices in OpenKrisp and their call app, then save the updated profiles.
There is no automatic migration or fallback. The setup panel exposes one microphone
and one speaker, including installation guidance when a driver is missing. The Cargo
routing feature is now `macos-virtual-devices`.

The native listening example now sends its stereo fixture into OpenKrisp Speaker's
channels 1–2 with channels 3–16 silent, then exercises the production incoming route.
It uses direct CPAL only for this test sender; production output negotiation remains
mono/stereo. The existing pinned CPAL dependency is additionally listed as an engine
dev dependency. This updated example has been compiled but not run on installed drivers.

The installer creates `OpenKrispMicrophone.driver` and `OpenKrispSpeaker.driver` under
`/Library/Audio/Plug-Ins/HAL`, with identifiers `app.openkrisp.audio.microphone` and
`app.openkrisp.audio.speaker`. Both are universal ARM64/x86_64. Driver packaging has
its own version (`1.0.0` initially), separate from upstream source and app versions.
Installed same/newer driver versions are reused independently; missing bundles or
executables are repaired. Official BlackHole bundles and receipts are untouched.
No installation script kills CoreAudio, changes default devices, or modifies other
HAL bundles. Installer restart metadata requests a reboot when a driver is selected.

Development drivers are ad-hoc signed; release drivers require a Developer ID
Application certificate and the combined package requires Developer ID Installer
signing and notarization. Original source, license, exact builder, manifest and icon
are included with the app's third-party notices. The existing project licensing and
release signing/notarization work remains open. See [installation](INSTALLATION.md).

## Linux name

The existing outgoing source already uses OpenKrisp Microphone. The incoming sink
now uses OpenKrisp Speaker in its PipeWire `node.description` and endpoint metadata.
All PipeWire node IDs, profile IDs, creation/Stop ownership and channel counts remain
unchanged. The description takes effect on the next creation of an owned incoming
route; pre-existing externally owned nodes are not renamed.

**Linux hardware and conferencing remain untested.** Deterministic policy checks
on macOS and historical Linux CI/private-graph checks do not establish that acceptance.

## Verification

Local validation on macOS ARM64 (2026-09-18):

| Check | Result |
| --- | --- |
| `cargo ci-test --locked` | 225 passed, 2 existing timing benchmarks ignored |
| Workspace/all-target/all-feature Clippy, warnings denied | Passed |
| Engine Clippy with AEC disabled | Passed |
| `cargo ci-fmt` | Passed |
| Frontend tests | 53 passed |
| Frontend production build | Passed |
| Python installer/quality scripts | 11 passed |
| Tauri release `.app` build | Passed |
| Combined development installer build and verification | Passed |
| `git diff --check` | Passed |

New Rust tests cover both named devices, strict discovery, channel-width validation,
obsolete endpoint rejection, independent recovery/Stop, no implicit migration/fallback, mirror
feedback rejection and the Linux sink's stable identity/name. Frontend tests cover
setup names, route requests, profile restoration, rejection of obsolete selections and missing named devices. Installer
JavaScript tests cover independently missing/old/current/newer drivers, mismatched
identifiers, missing executables and pre-existing BlackHole installations.

The package verifier loads both packaged HAL plugins together in an isolated process
and checks their actual CoreAudio properties: exact names and UIDs, channel counts in
both directions, virtual transport and hidden mirror identities. It also verifies both
Mach-O architecture slices, signatures, restart choices and packaged matching source/
build inputs. Native property execution was on ARM64; the x86_64 slice was built and
inspected. This does not install a driver or contact the audio server.

Logs are retained in `target/device-names-*.log`. The review artifact is
`target/release/bundle/pkg/OpenKrisp-devices-only-dev.pkg` (unsigned outer development
package, ad-hoc signed drivers), containing the new release app. The final verification uses the rebuilt release app with compatibility removed.
No GitHub CI run or installed-driver call acceptance is claimed for this branch.

Installer SHA-256: `2943df96008c863849404d9e46a07308022ad22dfd0deba119a93421d0d74c2b`.

## Remaining acceptance

- Install the new package, restart as requested, and confirm both names in macOS,
  OpenKrisp and Google Meet; explicitly select both devices in both apps.
- Repeat outgoing and incoming listening, stereo playback, overlapping speech and
  AEC status checks with the new driver builds. The earlier successful
  [macOS call report](GOOGLE_MEET_USER_TEST_2026-09-18.md) predates them.
- Exercise real install/repair/upgrade behavior and recovery/restart, plus the existing
  two-hour, Bluetooth, broader acoustic and hardware matrix criteria.
- Test Linux physical hardware and calls separately. No Linux manual result is claimed.
