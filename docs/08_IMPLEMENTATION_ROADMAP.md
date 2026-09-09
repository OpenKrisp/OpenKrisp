# Implementation Roadmap

## 1. Delivery strategy

Implement the system vertically in small phases. Every phase must leave the repository buildable and testable. Do not begin virtual-device or system-loopback work before the physical microphone-monitor path is stable.

## Phase 0: Repository and dependency spike

### Goals

- Establish the workspace and CI.
- Validate that CPAL and the pinned DeepFilterNet Rust integration build on target development machines.
- Discover actual model metadata and API constraints.

### Tasks

- Create the workspace and foundational crates.
- Add formatting, linting, and test commands.
- Create a minimal CPAL device-listing binary or example.
- Pin a DeepFilterNet revision and model asset.
- Build a minimal offline program that loads the model and processes a known frame.
- Record sample rate, channel layout, frame size, algorithmic latency, and thread-safety constraints.
- Add `ARCHITECTURE_VERIFICATION.md` with findings that differ from assumptions.

### Definition of done

- Workspace builds on the primary development platform.
- CPAL enumerates devices.
- DeepFilterNet loads and processes silence plus a fixture.
- Exact dependency revisions are locked.
- No architectural assumption remains unverified where it affects public types.

## Phase 1: Pure DSP and fake backend

### Goals

Build all data-path utilities without physical audio devices.

### Tasks

- Implement sample/channel layout types.
- Implement channel mapping.
- Implement model frame assembler.
- Integrate a preallocated resampler.
- Implement dry delay and crossfade.
- Implement limiter and meters.
- Implement bounded input/output ring wrappers.
- Create fake source and sink with deterministic callback sizes and clocks.
- Add a fake `NoiseReducer` with configurable processing delay and failures.

### Definition of done

- Random chunk sizes preserve sample order and duration.
- Resampling tests pass for required device rates.
- Bypass stays latency-aligned.
- Fake clock drift can be simulated.
- Steady-state DSP tests show no unexpected allocation in tested loops where measurement tooling permits.

## Phase 2: Headless microphone monitor

### Goals

Run the complete physical microphone-to-headphones route without Tauri.

### Tasks

- Implement CPAL device inventory.
- Implement input/output format negotiation.
- Build CPAL callbacks using bounded rings.
- Implement the DSP worker and model construction inside the worker thread.
- Add route state and supervisor lifecycle.
- Start output with prefill, then input.
- Add metrics and a CLI diagnostic display.
- Add clean shutdown.

### Definition of done

- User can choose input and output devices from CLI flags or prompts.
- Cleaned audio is audible.
- Route runs for at least two hours in a soak test without increasing latency.
- Temporary underruns do not crash the route.
- All route-owned threads stop and join.

## Phase 3: Robustness and adaptive drift

### Goals

Make the headless engine resilient to real device behavior.

### Tasks

- Implement adaptive output resampling based on ring occupancy.
- Add bounded overflow and underrun policies.
- Add model reset after discontinuities.
- Add sustained-overload detection and aligned bypass.
- Add stream-error reporting and supervisor recovery.
- Handle sleep/resume and common device reconnect scenarios where platform APIs permit.
- Add percentile timing metrics.

### Definition of done

- Separate USB input/output devices run without long-term queue drift.
- Simulated drift converges to the target fill.
- Device loss produces a controlled state transition.
- Repeated model deadline misses enter degraded mode rather than accumulating latency.

Implementation status: E9 is complete in software. The output path uses occupancy feedback and
bounded adjustable resampling; relevant ring discontinuities reset temporal state; sustained
deadline misses enter an observable aligned bypass with sparse recovery probes. Deterministic
±100 ppm and ±750 ppm clock simulations converge near target. The separate-device long-duration
hardware criterion remains open until it is exercised on the supported device matrix; see
`PHASE_3_DRIFT_OVERLOAD_IMPLEMENTATION_REPORT.md`.

## Phase 4: Tauri desktop MVP

### Goals

Expose the stable engine through a desktop UI.

### Tasks

- Create Tauri v2 + Vite + TypeScript app.
- Add `AppState` with `AudioEngineHandle`.
- Implement commands and DTO validation.
- Implement state, metrics, device, and fault events.
- Build device selectors, start/stop, bypass, meters, and diagnostics UI.
- Add profile persistence and schema versioning.
- Add tray and shutdown behavior.
- Configure least-privilege Tauri capabilities.

### Definition of done

- UI controls the engine without transporting PCM.
- Restarting the UI reconciles state through an engine snapshot.
- Metrics update without audible impact.
- Invalid commands return typed user-facing errors.
- Closing or quitting follows the documented lifecycle.

Implementation status: the command/event bridge slice is complete. The desktop application owns a
bounded engine service, revisioned reconciliation snapshots, validated commands, 10 Hz metrics,
versioned profiles, tray/quit lifecycle, and narrowly scoped Tauri permissions. A minimal React
shell proves the bridge, and Phase 5 now adds the route controls, meters and basic diagnostics needed for virtual
microphone operation. Profile management, denoise editing and full diagnostics remain in E12; see `PHASE_4_CONTROL_PLANE_IMPLEMENTATION_REPORT.md`.

## Phase 5: Virtual microphone integration

### Goals

Expose cleaned microphone audio to other applications.

### Tasks

- Implement capability discovery in `platform-routing`.
- Integrate one documented existing virtual endpoint on the primary platform.
- Add endpoint setup UI and validation.
- Route processed output to the virtual endpoint.
- Detect unsupported/missing endpoint states.
- Add loop-prevention checks.

### Definition of done

- A conferencing or recording application can select the virtual microphone.
- Start/stop leaves no stale route.
- Missing driver/endpoint yields clear instructions.
- Feedback-loop tests pass for the supported graph.

Implementation status: Phase 5 has BlackHole integration on macOS, native PipeWire endpoint
creation/routing on Linux. Windows support and the experimental adapter have been removed.
Physical/receiving-app acceptance on macOS/Linux remains pending. See `WINDOWS_SUPPORT.md`,
`PHASE_5_CROSS_PLATFORM_REPORT.md` and `PHASE_5_HARDWARE_TEST.md`. E14-06 remains open.

## Phase 6: Incoming speech processing

### Goals

Clean speech played through the application or captured through loopback.

### Tasks

- Add an application-owned PCM source first.
- Add explicit `SpeechDenoise` versus bypass mode.
- Validate mono/stereo behavior.
- Add platform loopback source behind the routing abstraction.
- Prevent recapture of processed output.
- Add route priority so microphone processing survives CPU pressure.

### Definition of done

- Speech fixtures and live speech are processed without corrupting duration or channel order.
- Music and non-speech can bypass by default.
- Loopback routing does not recurse.
- Concurrent microphone and speaker routes remain stable on supported hardware.

Implementation status: E15-01 through E15-07 are implemented in software. Native PCM and
loopback incoming routes default to bypass, preserve stereo and yield inference to microphone
pressure. Linux's private PipeWire test passes concurrent transfer and independent cleanup.
BlackHole 2ch is exclusive to one virtual direction at a time on macOS. A short BlackHole-to-AirPods
routing/listening check passed on 2026-09-09; broader listening and concurrent hardware/soak
acceptance remain open. See `PHASE_6_INCOMING_SPEECH_IMPLEMENTATION_REPORT.md` and
`PHASE_6_HARDWARE_TEST.md`.

## Phase 7: Full-duplex and production hardening

### Goals

Prepare for broad distribution and future echo cancellation.

### Tasks

- Add render-reference timestamps and buffering.
- Integrate or prototype a separate AEC module.
- Expand installers and permission onboarding.
- Decide whether the engine moves to a background sidecar/service.
- Add crash recovery and diagnostic export.
- Expand the hardware/platform matrix.
- Add release signing and model-asset integrity checks in packaging.

### Definition of done

- Full-duplex use is stable with headphones.
- Open-speaker limitations are explicit until AEC is validated.
- Install/uninstall leaves the system in a clean state.
- Production telemetry is local, privacy-preserving, and actionable.

Implementation status: E16-01/E16-02 provide native submitted-render/raw-capture taps, bounded
reference buffering, per-stream timestamps, clock alignment and delay estimation. No AEC is
integrated and no new hardware acceptance is claimed. See
[the Phase 7 slice report](PHASE_7_RENDER_REFERENCE_IMPLEMENTATION_REPORT.md). E16-03 onward and
Phase 6's remaining hardware acceptance are open.

## 2. Recommended pull-request sequence

1. Workspace skeleton and CI.
2. Protocol/domain types.
3. Frame assembler and channel mapper.
4. Delay, crossfade, limiter, and meters.
5. Fake backend and fake denoiser.
6. DeepFilterNet offline adapter.
7. CPAL inventory and format negotiation.
8. CPAL ring-buffer callbacks.
9. Pipeline worker and supervisor.
10. Headless monitor CLI.
11. Drift controller and recovery.
12. Tauri command/event bridge.
13. Tauri UI.
14. Platform routing integrations.

Each pull request should include focused tests and update the backlog status.

## 3. Exit criteria before virtual routing

Do not begin custom virtual-device work until:

- The physical monitor route is reliable.
- Drift compensation is proven with independent devices.
- Start/stop and recovery are deterministic.
- The engine is testable without Tauri.
- DeepFilterNet errors have an aligned fallback.
- Metrics can diagnose callback and worker problems.
