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
  - Tests: Runtime metadata checks and negative `Send`/`Sync` compile-time assertions in `offline_inference`.
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
- [ ] **E3-02** Implement model manifest and SHA-256 validation.
- [ ] **E3-03** Implement DeepFilterNet model loader.
- [ ] **E3-04** Implement reusable model input/output buffers.
- [ ] **E3-05** Implement exact-frame processing and dimension checks.
- [ ] **E3-06** Implement supported runtime setting updates.
- [ ] **E3-07** Implement reset and warm-up behavior.
- [ ] **E3-08** Implement non-finite output detection and typed errors.
- [x] **E3-09** Implement fake denoiser with delay/failure injection.
- [ ] **E3-10** Add fixture and repeated-frame tests.

**Depends on:** E0, E1, selected parts of E2.

Phase 1 scope completed E3-01 and E3-09. The production DeepFilterNet adapter and its fixture suite
remain intentionally open for the next implementation increment.

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

- [ ] **E6-01** Implement host and device inventory.
- [ ] **E6-02** Implement stable device descriptors and default flags.
- [ ] **E6-03** Implement input format negotiation.
- [ ] **E6-04** Implement output format negotiation.
- [ ] **E6-05** Implement sample-type conversion helpers.
- [ ] **E6-06** Implement minimal non-blocking input callback.
- [ ] **E6-07** Implement minimal non-blocking output callback.
- [ ] **E6-08** Implement compact stream-error forwarding.
- [ ] **E6-09** Add manual hardware test protocol.

**Depends on:** E0, E1, E4.

## Epic E7: Pipeline worker

- [ ] **E7-01** Define worker initialization and readiness handshake.
- [ ] **E7-02** Construct DeepFilterNet inside the worker.
- [ ] **E7-03** Connect input mapping and input resampling.
- [ ] **E7-04** Connect frame assembler and denoiser.
- [ ] **E7-05** Connect dry delay, crossfade, limiter, and meters.
- [ ] **E7-06** Connect output resampling and output ring.
- [ ] **E7-07** Implement bounded worker control commands.
- [ ] **E7-08** Implement cancellation and deterministic shutdown.
- [ ] **E7-09** Implement inference timing metrics.
- [ ] **E7-10** Implement model-error aligned fallback.

**Depends on:** E2, E3, E4.

## Epic E8: Supervisor and route lifecycle

- [ ] **E8-01** Implement authoritative route state machine.
- [ ] **E8-02** Implement start sequence and output prefill.
- [ ] **E8-03** Implement stop and thread join sequence.
- [ ] **E8-04** Implement duplicate/invalid-state command rejection.
- [ ] **E8-05** Implement stream fault handling.
- [ ] **E8-06** Implement route recovery budget and transitions.
- [ ] **E8-07** Implement engine snapshot store.
- [ ] **E8-08** Implement bounded engine command channel.
- [ ] **E8-09** Implement engine shutdown.

**Depends on:** E5, E6, E7.

## Epic E9: Drift control and overload behavior

- [ ] **E9-01** Implement output-fill feedback controller.
- [ ] **E9-02** Integrate adjustable resampling ratio or equivalent strategy.
- [ ] **E9-03** Bound and expose drift correction in ppm.
- [ ] **E9-04** Implement discontinuity detection and state reset.
- [ ] **E9-05** Implement sustained model-deadline detection.
- [ ] **E9-06** Implement degraded aligned-bypass mode.
- [ ] **E9-07** Add positive/negative drift simulations.
- [ ] **E9-08** Add CPU-spike and overload tests.

**Depends on:** E5, E7, E8.

## Epic E10: Headless monitor application

- [ ] **E10-01** Add CLI device listing.
- [ ] **E10-02** Add CLI route configuration.
- [ ] **E10-03** Display state and metrics.
- [ ] **E10-04** Add signal handling and clean shutdown.
- [ ] **E10-05** Run and document a two-hour soak test.
- [ ] **E10-06** Record baseline latency and inference measurements.

**Depends on:** E6, E7, E8, E9.

## Epic E11: Tauri control plane

- [ ] **E11-01** Initialize Tauri v2 + Vite + TypeScript UI.
- [ ] **E11-02** Add `AppState` and engine startup.
- [ ] **E11-03** Implement device and snapshot commands.
- [ ] **E11-04** Implement start/stop/update/bypass commands.
- [ ] **E11-05** Implement DTO validation and error conversion.
- [ ] **E11-06** Implement throttled state, metrics, and fault events.
- [ ] **E11-07** Implement configuration persistence and migration.
- [ ] **E11-08** Implement tray and shutdown behavior.
- [ ] **E11-09** Configure least-privilege Tauri capabilities.

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
