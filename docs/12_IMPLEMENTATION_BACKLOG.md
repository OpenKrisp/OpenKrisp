# Implementation Backlog

This backlog is organized into epics and ordered by dependency. An autonomous coding assistant should take one epic or a small set of closely related tasks per assignment.

## Epic E0: Architecture verification and workspace

- [x] **E0-01** Create the Rust workspace and Tauri application directories.
  - Completed in commit/PR: Current Phase 0 change set.
  - Tests: `cargo metadata --format-version 1 --locked`.
  - Notes: All documented crate boundaries exist; the Tauri runtime remains intentionally deferred to Phase 4.
- [x] **E0-02** Add `rust-toolchain.toml`, formatting, Clippy, and test configuration.
  - Completed in commit/PR: Current Phase 0 change set.
  - Tests: `cargo ci-fmt`, `cargo ci-clippy`, and `cargo ci-test --locked`.
  - Notes: Rust is pinned to latest stable 1.98.0 with unsafe code denied in project crates.
- [x] **E0-03** Add CI for Rust formatting, Clippy, tests, and frontend type checking.
  - Completed in commit/PR: Current Phase 0 change set.
  - Tests: Workflow syntax inspection plus equivalent local Rust and npm commands.
  - Notes: CI covers Linux, macOS, Windows, offline inference, frontend types, and dependency licenses.
- [x] **E0-04** Pin CPAL and create a device-enumeration example.
  - Completed in commit/PR: Current Phase 0 change set.
  - Tests: `cargo run --locked -p audio-cpal --example list_devices`.
  - Notes: CPAL 0.18.2 enumerated five CoreAudio devices and their default formats on the primary machine.
- [x] **E0-05** Pin DeepFilterNet and its low-latency model asset.
  - Completed in commit/PR: Current Phase 0 change set.
  - Tests: SHA-256 validation in the offline inference example.
  - Notes: Upstream v0.5.6 commit and the official low-latency ONNX archive are exact pins.
- [x] **E0-06** Create an offline DeepFilterNet inference spike.
  - Completed in commit/PR: Current Phase 0 change set.
  - Tests: `cargo run --locked -p denoise-dfn --example offline_inference`.
  - Notes: The spike processes silence and a deterministic non-silent synthetic fixture with finite output.
- [x] **E0-07** Verify model rate, frame size, layout, latency, settings, and thread traits.
  - Completed in commit/PR: Current Phase 0 change set.
  - Tests: Runtime metadata checks and negative `Send`/`Sync` compile-time assertions in the production adapter.
  - Notes: Verified values and upstream limitations are recorded in `docs/ARCHITECTURE_VERIFICATION.md`.
- [x] **E0-08** Write `docs/ARCHITECTURE_VERIFICATION.md`.
  - Completed in commit/PR: Current Phase 0 change set.
  - Tests: Reviewed against the pinned source, runtime spike output, and local CPAL inventory.
  - Notes: The report separates source-verified capabilities from hardware-tested behavior.
- [x] **E0-09** Verify dependency licenses and record release-relevant findings.
  - Completed in commit/PR: Current Phase 0 change set.
  - Tests: `cargo deny check licenses` and locked Cargo/npm metadata inspection.
  - Notes: Current dependencies are permissively licensed; final model redistribution still requires release legal review.

**Depends on:** nothing.

## Epic E1: Shared protocol and domain types

- [x] **E1-01** Implement `RouteId`, `DeviceId`, and endpoint newtypes.
- [x] **E1-02** Implement `AudioFormat` and explicit unit-bearing fields.
- [x] **E1-03** Implement route configuration and processing-mode types.
- [x] **E1-04** Implement route-state and event types.
- [x] **E1-05** Implement metrics snapshot types.
- [x] **E1-06** Implement stable internal and UI-facing error codes.
- [x] **E1-07** Add serialization round-trip tests for DTO-safe types.

Phase 1 evidence: identifiers reject invalid wire values, explicit-unit fields reject zero where
required, and route/configuration/event/metrics/error payloads have JSON round-trip tests.

**Depends on:** E0.

## Epic E2: Pure DSP primitives

- [x] **E2-01** Implement interleaved sample and channel-layout helpers.
- [x] **E2-02** Implement channel mapper with documented policies.
- [x] **E2-03** Implement model-frame assembler with arbitrary chunk support.
- [x] **E2-04** Select and integrate a preallocatable resampler.
- [x] **E2-05** Implement fixed dry delay line.
- [x] **E2-06** Implement click-resistant wet/dry crossfade.
- [x] **E2-07** Implement output limiter/non-finite protection.
- [x] **E2-08** Implement peak and RMS meters.
- [x] **E2-09** Add randomized chunking tests.
- [x] **E2-10** Add resampling duration and reset tests.

Phase 1 evidence: deterministic tests cover mono/stereo policies, arbitrary stereo chunking, aligned
bypass, non-finite safety, meter decay, 44.1/48/96 kHz conversion, reset, and measured
allocation-free steady-state loops.

**Depends on:** E0, E1.

## Epic E3: Denoiser abstraction and DeepFilterNet adapter

- [x] **E3-01** Create backend-neutral `NoiseReducer` trait.
- [x] **E3-02** Implement model manifest and SHA-256 validation.
- [x] **E3-03** Implement DeepFilterNet model loader.
- [x] **E3-04** Implement reusable model input/output buffers.
- [x] **E3-05** Implement exact-frame processing and dimension checks.
- [x] **E3-06** Implement supported runtime setting updates.
- [x] **E3-07** Implement reset and warm-up behavior.
- [x] **E3-08** Implement non-finite output detection and typed errors.
- [x] **E3-09** Implement fake denoiser with delay/failure injection.
- [x] **E3-10** Add fixture and repeated-frame tests.

**Depends on:** E0, E1, selected parts of E2.

Phase 2 evidence: the adapter validates the packaged manifest, exact upstream revision, and streamed
SHA-256 before loading; owns reusable planar arrays; checks model-derived dimensions and finite
audio; applies the two verified runtime controls; and reconstructs state after warm-up/reset. Tests
cover missing/tampered assets, silence, a deterministic non-silent fixture repeated across frames,
settings, dimensions, finite output, and reconstruction.

## Epic E4: Bounded audio transport

- [x] **E4-01** Select or implement a bounded SPSC audio ring.
- [x] **E4-02** Add frame-aware ring wrapper types.
- [x] **E4-03** Implement slice-based non-blocking read/write.
- [x] **E4-04** Implement fill, high-water, overrun, and underrun metrics.
- [x] **E4-05** Implement input drop-oldest/stale policy.
- [x] **E4-06** Implement output safe-fill policy.
- [x] **E4-07** Add capacity and randomized producer/consumer tests.

Phase 1 evidence: the `ringbuf` wrapper has fixed frame capacity, complete-frame slice I/O, bounded
excess-input drops, consumer-owned stale trimming, safe output silence fill, atomic health metrics,
randomized ordering tests, and a measured allocation-free steady-state loop.

**Depends on:** E1.

## Epic E5: Fake backend and deterministic simulation

- [x] **E5-01** Implement deterministic fake audio source.
- [x] **E5-02** Implement deterministic fake audio sink.
- [x] **E5-03** Support configurable callback sizes and jitter.
- [x] **E5-04** Support independent input/output clock drift.
- [x] **E5-05** Support device loss and stream-error injection.
- [x] **E5-06** Build a simulated route harness.
- [x] **E5-07** Add lifecycle and queue-bounds tests.

Phase 1 evidence: timestamp-ordered simulations cover cyclic callback patterns, deterministic
jitter, positive/negative drift, injected reducer failures, stream errors, device loss, duplicate
start rejection, clean stop, and fixed queue bounds.

**Depends on:** E1, E2, E3, E4.

## Epic E6: CPAL adapter

- [x] **E6-01** Implement host and device inventory.
- [x] **E6-02** Implement stable device descriptors and default flags.
- [x] **E6-03** Implement input format negotiation.
- [x] **E6-04** Implement output format negotiation.
- [x] **E6-05** Implement sample-type conversion helpers.
- [x] **E6-06** Implement minimal non-blocking input callback.
- [x] **E6-07** Implement minimal non-blocking output callback.
- [x] **E6-08** Implement compact stream-error forwarding.
- [x] **E6-09** Add manual hardware test protocol.

Phase 2 evidence: `audio-cpal` exposes backend-neutral inventory DTOs, opaque device-ID resolution,
deterministic mono/stereo PCM negotiation, typed conversion for every CPAL PCM representation, and
preallocated chunked callbacks. Callback code performs conversion, bounded ring I/O, worker wake,
and atomic accounting only. Hardware acceptance steps are in `PHASE_2_HARDWARE_TEST.md`.

**Depends on:** E0, E1, E4.

## Epic E7: Pipeline worker

- [x] **E7-01** Define worker initialization and readiness handshake.
- [x] **E7-02** Construct DeepFilterNet inside the worker.
- [x] **E7-03** Connect input mapping and input resampling.
- [x] **E7-04** Connect frame assembler and denoiser.
- [x] **E7-05** Connect dry delay, crossfade, limiter, and meters.
- [x] **E7-06** Connect output resampling and output ring.
- [x] **E7-07** Implement bounded worker control commands.
- [x] **E7-08** Implement cancellation and deterministic shutdown.
- [x] **E7-09** Implement inference timing metrics.
- [x] **E7-10** Implement model-error aligned fallback.

Phase 2 evidence: the reducer factory executes inside the named worker and reports readiness only
after model construction and warm-up. The fixed graph connects mapping, both resamplers, exact
model framing, aligned wet/dry processing, safety, metrics, and bounded output. Tests deliberately
use a non-`Send` reducer, cross 44.1/48/96 kHz and stereo/mono boundaries, fill the bounded command
queue, join the worker, and verify delayed dry output after model failures.

**Depends on:** E2, E3, E4.

## Epic E8: Supervisor and route lifecycle

- [x] **E8-01** Implement authoritative route state machine.
- [x] **E8-02** Implement start sequence and output prefill.
- [x] **E8-03** Implement stop and thread join sequence.
- [x] **E8-04** Implement duplicate/invalid-state command rejection.
- [x] **E8-05** Implement stream fault handling.
- [x] **E8-06** Implement route recovery budget and transitions.
- [x] **E8-07** Implement engine snapshot store.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: immutable revision and full JSON round-trip tests.
  - Notes: callbacks never access the store; the supervisor publishes low-rate cloned snapshots.
- [x] **E8-08** Implement bounded engine command channel.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: deterministic queue saturation and pre-enqueue validation tests.
  - Notes: submission is non-blocking and returns a typed busy or disconnected error.
- [x] **E8-09** Implement engine shutdown.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: active-route teardown, failed-route stop, idempotent owner shutdown, and thread joins.
  - Notes: an atomic shutdown signal remains available even when the regular command queue is full.

Phase 2 provided the initial single-route headless supervisor. The Phase 3 recovery slice completes
E8-06 with explicit portable fault classification, a finite exponential-backoff budget,
`Reconfiguring`/`Recovering` transitions, generation rebuilds, activation/settling checks, and
cumulative recovery diagnostics. Input-first startup and post-activation native-format negotiation
also handle the observed CoreAudio Bluetooth duplex profile change without retuning the device
clock. The Phase 4 control-plane slice completes E8-07 through E8-09 with an application-owned
engine service; see `PHASE_4_CONTROL_PLANE_IMPLEMENTATION_REPORT.md`.

**Depends on:** E5, E6, E7.

## Epic E9: Drift control and overload behavior

- [x] **E9-01** Implement output-fill feedback controller.
- [x] **E9-02** Integrate adjustable resampling ratio or equivalent strategy.
- [x] **E9-03** Bound and expose drift correction in ppm.
- [x] **E9-04** Implement discontinuity detection and state reset.
- [x] **E9-05** Implement sustained model-deadline detection.
- [x] **E9-06** Implement degraded aligned-bypass mode.
- [x] **E9-07** Add positive/negative drift simulations.
- [x] **E9-08** Add CPU-spike and overload tests.

The Phase 3 drift/overload slice uses a filtered, slew-limited PI controller and an adjustable
output sinc resampler even when both endpoints advertise the same nominal rate. Corrections are
bounded to 1,000 ppm and published with queue health. Ring discontinuities reset temporal DSP and
model state. Four misses in an eight-frame window enter aligned dry bypass; degraded operation
skips seven of every eight model calls and uses healthy probes to recover. Deterministic ±100 ppm
and ±750 ppm simulations, real delayed-inference worker tests, and allocation instrumentation pass.
Physical long-duration validation remains separate; see
`PHASE_3_DRIFT_OVERLOAD_IMPLEMENTATION_REPORT.md`.

**Depends on:** E5, E7, E8.

## Epic E10: Headless monitor application

- [x] **E10-01** Add CLI device listing.
- [x] **E10-02** Add CLI route configuration.
- [x] **E10-03** Display state and metrics.
- [x] **E10-04** Add signal handling and clean shutdown.
- [ ] **E10-05** Run and document a two-hour soak test.
- [ ] **E10-06** Record baseline latency and inference measurements.

The Phase 2 CLI selects opaque input/output IDs, model path, startup latency, verified denoise
settings, and bypass; publishes one-second health snapshots; and handles Ctrl-C or a bounded test
duration. A 10-second built-in-microphone-to-headphones smoke run confirmed real callbacks,
processing, temporary-underrun recovery, and clean shutdown. E10-05 and E10-06 stay open until the
full manual hardware protocol is run on the supported OS/device matrix.

**Depends on:** E6, E7, E8, E9.

## Epic E11: Tauri control plane

- [x] **E11-01** Initialize Tauri v2 + Vite + TypeScript UI.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: production frontend build and strict TypeScript typecheck.
  - Notes: Tauri v2, Vite, React, and exact dependency versions are locked.
- [x] **E11-02** Add `AppState` and engine startup.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: desktop crate all-target check and runtime launch smoke test.
  - Notes: `AppState` owns only the engine handle, profiles, and deterministic lifecycle resources.
- [x] **E11-03** Implement device and snapshot commands.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: command registration build plus engine snapshot serialization tests.
  - Notes: device refresh is queued; reads clone the latest authoritative snapshot.
- [x] **E11-04** Implement start/stop/update/bypass commands.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: fake-driver start/update/bypass/stop lifecycle test.
  - Notes: commands acknowledge bounded enqueueing; asynchronous results arrive through events or snapshots.
- [x] **E11-05** Implement DTO validation and error conversion.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: invalid identifier, latency, denoise, and serialized error-shape tests.
  - Notes: backend details and local paths are converted to stable user-facing errors.
- [x] **E11-06** Implement throttled state, metrics, and fault events.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: stable topic mapping and frontend event-reducer tests.
  - Notes: metrics are capped at 10 Hz and a bounded queue may drop redundant events; snapshots reconcile state.
- [x] **E11-07** Implement configuration persistence and migration.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: schema migration, atomic reopen, invalid input, and non-overwrite fallback tests.
  - Notes: validated schema-v1 profile mutations are serialized and atomically replace the config file.
- [x] **E11-08** Implement tray and shutdown behavior.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: icon construction, engine teardown, and desktop runtime launch smoke test.
  - Notes: window close hides to tray; explicit quit joins the route, supervisor, and event bridge.
- [x] **E11-09** Configure least-privilege Tauri capabilities.
  - Completed in commit/PR: Phase 4 control-plane change set.
  - Tests: Tauri capability/schema validation during the desktop build.
  - Notes: the main window can only invoke named app commands and listen/unlisten to events.

Phase 4 control-plane evidence and remaining UI scope are documented in
`PHASE_4_CONTROL_PLANE_IMPLEMENTATION_REPORT.md`.

**Depends on:** E8, E10.

## Epic E12: Desktop UI

- [ ] **E12-01** Build device selectors.
- [ ] **E12-02** Build route start/stop and state display.
- [ ] **E12-03** Build bypass and supported denoise controls.
- [ ] **E12-04** Build input/output meters.
- [ ] **E12-05** Build diagnostics screen.
- [ ] **E12-06** Build profiles UI.
- [ ] **E12-07** Add frontend state reconciliation after reload.
- [ ] **E12-08** Add permission and unsupported-feature messaging.

**Depends on:** E11.

## Epic E13: Platform routing foundation

- [ ] **E13-01** Implement `PlatformAudioRouter` trait and capability DTOs.
- [ ] **E13-02** Implement no-op/physical-only router.
- [ ] **E13-03** Implement endpoint ancestry and loop validation model.
- [ ] **E13-04** Add platform-specific modules and feature gates.
- [ ] **E13-05** Surface capabilities in Tauri UI.

**Depends on:** E1, E8, E11.

## Epic E14: Virtual microphone

- [ ] **E14-01** Select one existing virtual endpoint on the primary platform.
- [ ] **E14-02** Implement endpoint discovery.
- [ ] **E14-03** Implement processed-output route to virtual endpoint.
- [ ] **E14-04** Add setup and missing-endpoint UX.
- [ ] **E14-05** Add feedback-loop validation.
- [ ] **E14-06** Test with at least one conferencing or recording application.

**Depends on:** E10, E12, E13.

## Epic E15: Incoming speech and loopback

- [ ] **E15-01** Add application-owned PCM source abstraction.
- [ ] **E15-02** Define stereo/multi-channel DeepFilterNet policy.
- [ ] **E15-03** Implement speech-only route mode and bypass default.
- [ ] **E15-04** Implement primary-platform loopback source.
- [ ] **E15-05** Prevent processed-output recapture.
- [ ] **E15-06** Add concurrent route priority/degradation policy.
- [ ] **E15-07** Add music bypass and speech fixture tests.

**Depends on:** E9, E13, E14 where virtual routing is required.

## Epic E16: Full-duplex and AEC readiness

- [ ] **E16-01** Add render-reference interface.
- [ ] **E16-02** Add audio timestamps and delay estimation plumbing.
- [ ] **E16-03** Evaluate AEC implementations and licenses.
- [ ] **E16-04** Prototype AEC before DeepFilterNet in microphone path.
- [ ] **E16-05** Add open-speaker echo tests.
- [ ] **E16-06** Keep headphones as documented fallback until acceptance criteria pass.

**Depends on:** E15.

## Epic E17: Production hardening

- [ ] **E17-01** Expand macOS/Windows/Linux hardware matrix.
- [ ] **E17-02** Add packaged model integrity smoke test.
- [ ] **E17-03** Add installer permission/onboarding flow.
- [ ] **E17-04** Add sanitized diagnostic export.
- [ ] **E17-05** Evaluate background service/sidecar architecture.
- [ ] **E17-06** Add crash and restart recovery tests.
- [ ] **E17-07** Complete license and third-party notice review.
- [ ] **E17-08** Define supported hardware and latency profiles.

**Depends on:** E11-E16 as applicable.

## Task completion format

When completing backlog tasks, update the checkbox and append a short note beneath it:

```text
- [x] E2-03 Implement model-frame assembler with arbitrary chunk support.
  - Completed in commit/PR: <reference>
  - Tests: <test names>
  - Notes: <important constraints or follow-up>
```

Do not mark a task complete when only interfaces or placeholders exist unless the task explicitly asks for a scaffold.
