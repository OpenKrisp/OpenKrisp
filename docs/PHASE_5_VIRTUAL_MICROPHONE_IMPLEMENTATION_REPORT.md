# Phase 5: Virtual microphone implementation report

Historical record: device names, routing restrictions, controls and results below apply
to the build described here. For current setup and repeat testing, use the
[installation guide](INSTALLATION.md), [microphone hardware procedure](PHASE_5_HARDWARE_TEST.md),
and [incoming/full-duplex procedure](PHASE_6_HARDWARE_TEST.md). The current macOS graph
uses separate **OpenKrisp Microphone** and **OpenKrisp Speaker** drivers and supports
both routes concurrently; old BlackHole endpoint selections are unsupported.

This report records the initial macOS slice on 2026-09-07. The subsequent
[cross-platform report](PHASE_5_CROSS_PLATFORM_REPORT.md) supersedes its Windows/Linux support
status and describes current ownership, dependencies, validation and remaining release gates.

## Scope and result

Phase 5's software path is implemented: capability discovery, a macOS BlackHole 2ch adapter,
validated virtual destinations, processed CPAL output, setup UI, and feedback-loop checks.
E13-01 through E13-05 and E14-01 through E14-05 are implemented. E14-06 and the hardware
acceptance criteria remain open because BlackHole is not installed on the development host.
No recording/conferencing application audio test, subjective listening test, or long virtual
route soak is claimed.

The existing Phase 4 shell also gains the controls needed to operate this route: microphone and
output selection, refresh, start/stop, lifecycle state, bypass, peak meters, and basic diagnostics.
The remaining E12 profile management, denoise parameter editing, and full diagnostics UI remain
separate work.

## Implementation plan and boundaries

- Extend `audio-protocol` with native device kinds, routing capabilities, endpoint availability,
  ancestry, and a routing snapshot; keep these metadata-only.
- Implement a stateless `PlatformAudioRouter`, a physical-only fallback, and a feature-gated macOS
  adapter in `platform-routing`. Existing endpoint routing creates no persistent OS graph links.
- Fill the missing CPAL macOS transport metadata with a small, read-only native adapter. Keep its
  FFI confined to one module and off audio callbacks.
- Resolve the logical virtual destination to explicit CPAL IDs on the route owner and validate
  each recovery generation before opening input. Preserve all current DSP and callback behavior.
- Extend the existing start command with a destination discriminator; surface discovery in the
  authoritative snapshot rather than adding commands or permissions.
- Test identity, missing/invalid devices, ancestry, unsupported builds, restart/cleanup, recovery
  policy, serialization, IPC, setup UI and frontend selection validation.

## Supported graph and endpoint contract

```text
Verified physical microphone -> existing Rust denoise pipeline
                            -> BlackHole 2ch output, channels 1–2
                            -> receiving app's BlackHole 2ch input
Receiving app's speakers     -> physical headphones
```

The adapter supports the standard BlackHole 2ch device only. The logical ID is
`macos:blackhole-2ch`; its CPAL device ID is `coreaudio:BlackHole2ch_UID`. Discovery requires
CoreAudio, the exact stable UID, native virtual classification, both capture and playback roles,
and stereo default formats. Display names are not used for identity. A renamed standard endpoint
is supported; renamed custom drivers, 16/64-channel variants, split mirror configurations and
aggregate/multi-output routes are outside this integration.

The optional `coreaudio:BlackHole2ch_2_UID` mirror shares the original recursive path and is
recognized for loop rejection. The standard public endpoint is the only output offered.
The existing mono worker output is duplicated to the endpoint's stereo output by the existing
channel mapper. Native rate conversion and adaptive drift compensation stay in the existing worker.

Capabilities distinguish implemented integration from endpoint availability (`ready`, `missing`,
`invalid`, `unsupported`). At this historical implementation stage, other platforms and builds without the `macos-blackhole` feature exposed
physical routing only. The macOS policy module is host-independent Rust so its graph tests also
run on non-macOS CI.

## Ownership, validation, and recovery

Platform preparation returns identifiers and creates no driver, graph link, stream or model.
It does not change system defaults or install/uninstall software. The CPAL route owner retains
all live streams and the existing worker; Stop and Quit use its existing cancellation/join path.
Release is idempotent because the installed endpoint belongs to the user, not to an OpenKrisp route.

Preparation runs on the route owner before model construction/capture, using a fresh inventory.
Every startup/recovery generation validates its original logical request and exact selected
input/output IDs again. Device/default changes cannot redirect output to a different endpoint.
Missing endpoints and devices use the finite recovery budget; feedback, invalid endpoints and
unverifiable graphs fail immediately. Startup routing failures become sanitized `routing` faults
with installation or microphone-selection instructions. Recovery exhaustion retains the existing
stream-fault UI with reconnect/restart guidance.

Virtual microphone sources must have verified physical transport. A known shared recursive path
fails first; incomplete ancestry also fails closed. Aggregate and unknown transports are rejected
for virtual routes rather than inferred from their display names. Labeling BlackHole as a physical
source/output cannot evade the macOS router's UID checks. These checks cover the supported graph;
OpenKrisp does not inspect or control external applications' complete audio graphs, implement AEC,
or guarantee safety for arbitrary externally constructed loops.

No audio callback, ring, resampler, denoiser or inference code is modified by this phase. Native
metadata queries and allocations are control-plane work only.

## Public API and native metadata decision

`EngineSnapshot.routing` and `AudioDeviceDescriptor.kind` are additive, with safe Rust serde
defaults for older snapshots. The existing `start_route` request accepts optional
`destinationKind: physical_output | virtual_microphone`; omitted values retain the Phase 4
physical-output behavior. For a virtual request, `destinationDeviceId` contains the logical
virtual endpoint ID. The resulting domain configuration retains `VirtualMicrophone` throughout
snapshots and lifecycle operations. Unknown destination-kind strings and unknown request fields
are rejected by serde. Native endpoint validation occurs asynchronously on the route owner,
consistent with the existing command acknowledgement contract.

No new Tauri command, capability, shell permission or network permission is added. The setup URL
is displayed for copying; the application does not download or execute a driver installer.
Profiles retain their existing schema and are not repurposed to store virtual endpoint IDs.

CPAL 0.18.2 provides stable CoreAudio UIDs and aggregate classification, but leaves normal physical
and virtual transport unspecified. `audio-cpal::macos_metadata` translates a stable UID to a native
device ID and reads its transport property. Failed queries classify the device as unknown. The
crate's `forbid(unsafe_code)` becomes `deny(unsafe_code)` with one explicitly allowed module.
Its two synchronous FFI calls have size/status checks and documented pointer lifetimes; all other
modules still deny unsafe code. No raw handles enter domain DTOs. The direct objc2 dependencies
are pinned to the same 0.3.2 versions already present through CPAL, with macOS-only dependencies.

## Validation evidence

Final automated result: **142 Rust tests passed**, with one pre-existing opt-in timing benchmark
ignored; **18 frontend tests passed**. The physical-only feature-disabled suite also passed its
two tests. Formatting, warnings-denied Clippy, TypeScript checking and the production frontend
build passed. The dependency-license gate also passed (`licenses ok`).

- Workspace formatting, warnings-denied Clippy and locked workspace tests.
- Platform router tests for exact identity, renamed devices, duplicate IDs, wrong roles/channel
  counts/transports/hosts, standard and mirrored recursion, physical-discriminator evasion,
  incomplete ancestry, missing devices, unsupported endpoints, 100 prepare/release cycles and
  router recreation after inventory/default changes.
- Physical-only router tests with the macOS integration feature disabled.
- Engine tests for virtual destination preservation, repeated start/stop, shutdown joins, actionable
  preparation failures, and missing-endpoint versus feedback recovery policy.
- Production Tauri invoke harness validates the explicit virtual destination and rejects unsupported
  discriminator strings. Existing command allowlist and no-PCM bridge checks still pass.
- Frontend type checking, production Vite build, IPC envelopes, selection validation, reload
  restoration, lifecycle guards, and rendered setup states.
- Native read-only enumeration on 2026-09-07 found five device entries, including built-in audio,
  a continuity microphone, Bluetooth output and a multi-output device. BlackHole was absent.
  The physical entries classified as `Physical`; the multi-output entry classified as `Aggregate`.

The ignored production-model wall-clock recovery benchmark remains explicitly opt-in. Existing
physical-monitor listening/soak and multi-device acceptance from Phases 2–3 remain pending; this
change does not replace those acceptance gates. Cross-platform compilation is left to the existing
Linux/macOS CI matrix. Windows support was subsequently removed.

## Hardware acceptance still required

Follow [the Phase 5 hardware test](PHASE_5_HARDWARE_TEST.md). Complete E14-06 only after an actual
receiving app selects BlackHole, receives cleaned speech, and stops receiving live microphone
audio after Stop/Quit. Repeat with built-in and USB microphones, default-device changes, endpoint
loss/reconnect, and a two-hour run; record negotiated formats and latency/queue metrics.

## Verified upstream references

Verified on 2026-09-07:

- [BlackHole installation and inter-application routing instructions](https://github.com/ExistentialAudio/BlackHole#installation-instructions).
- [BlackHole source: default UIDs, two-channel configuration and mirror behavior](https://github.com/ExistentialAudio/BlackHole/blob/master/BlackHole/BlackHole.c).
- CPAL 0.18.2's locked local `device_description.rs`, `DeviceId` implementation and
  `host/coreaudio/macos/device.rs` confirm stable ID serialization and missing transport metadata.

The BlackHole driver and its source are not copied, linked or bundled in this repository.
