# Phase 2 Implementation Report

## Scope

Roadmap Phase 2 adds the software path for a physical, headless microphone-to-headphones monitor.
It completes the production DeepFilterNet adapter (remaining E3 work), the CPAL boundary (E6), the
pipeline worker (E7), the single-route supervisor portion of E8, and the executable portions of
E10. Adaptive clock-drift control, recovery budgets, the multi-route engine handle, Tauri, and
platform virtual-device routing remain later phases.

A 10-second physical smoke test was run after implementation and confirmed local microphone
callbacks, model processing, output callbacks, temporary-underrun recovery, and clean shutdown.
Subjective listening acceptance and the two-hour soak were not completed; those gates require a
human listener and a controlled hardware session, so they remain explicitly open and have a
reproducible protocol in `PHASE_2_HARDWARE_TEST.md`.

## Production DeepFilterNet adapter

- `ModelAsset` loads the adjacent schema-versioned manifest, requires the pinned upstream revision,
  validates a single-file entry, streams SHA-256 over the archive, and rejects missing or tampered
  assets with typed errors.
- `DfnNoiseReducer` derives and verifies rate, channels, model-frame size, and algorithmic latency
  against the loaded runtime instead of trusting manifest dimensions alone.
- The adapter owns reusable `ndarray` input/output arrays and accepts exact caller-owned planar
  slices. It rejects wrong lengths and non-finite input or output without resizing buffers.
- Runtime updates are limited to the two controls verified at the pinned revision: attenuation
  limit and post-filter beta. Reset reconstructs recurrent state from the retained parameters.
- Warm-up uses a quiet deterministic non-zero signal because the pinned implementation
  short-circuits exact silence before inference; it then reconstructs the model so live state is
  clean.
- A compile-time negative assertion records that the concrete runtime is neither `Send` nor `Sync`.

## CPAL boundary

- `CpalHost` enumerates the default host into serializable, backend-neutral device descriptors with
  opaque IDs, direction capabilities, default flags, manufacturer, and default formats.
- Input and output negotiation deterministically prefer the canonical rate, requested mono/stereo
  layout, and `f32`, while accepting every CPAL integer/float PCM type. DSD and layouts above stereo
  are rejected for this route.
- Typed callbacks convert through one preallocated 2,048-frame scratch buffer and split larger
  backend callbacks into bounded chunks.
- Input callbacks only convert, write to the fixed SPSC ring, update atomics, and wake the worker.
  Output callbacks only read-or-silence, convert, and update atomics. Neither path logs, waits, does
  file I/O, constructs DSP, or intentionally allocates in steady state.
- CPAL's non-exhaustive error kinds are reduced to compact atomic counters. Xruns remain observable
  but non-terminal; all other stream-error callback categories fail the Phase 2 route cleanly.

## Worker and supervisor

The startup and shutdown ownership order is:

```text
validate + negotiate
  -> allocate rings and output prefill
  -> spawn worker; construct/warm model inside it; await readiness
  -> construct both CPAL streams
  -> play output, then input
  -> Running

Stopping or terminal fault
  -> drop input stream
  -> cancel and join worker
  -> drop output stream
  -> Stopped (or Failed until explicitly stopped)
```

The worker connects stereo-to-mono mapping, input resampling, exact model-frame assembly,
DeepFilterNet, algorithmically aligned dry delay, equal-power crossfade, finite-safe limiting,
levels, output resampling, mono-to-stereo mapping, and the bounded output ring. A bounded command
channel applies setting and bypass changes only between worker iterations. Cancellation is also an
atomic signal, so shutdown does not depend on space in that channel.

One model failure produces the delayed dry frame, requests a click-resistant transition to dry,
increments error metrics, and attempts model reconstruction. Stale input is trimmed to a fixed
target and resets temporal DSP state. Sustained-overload policy and automatic stream recovery are
reserved for Phase 3.

The supervisor owns the authoritative state machine and samples lock-free queue, callback, stream,
level, inference percentile, deadline, model reset/error, and latency metrics. It detects a stopped
worker or terminal stream issue, marks the route failed, and releases all route-owned resources.

## Headless CLI

`openkrisp-monitor` can:

- list physical devices and opaque selection IDs;
- select default or explicit input/output endpoints;
- override the model path and startup latency;
- set attenuation limit, disable the verified post-filter, or start bypassed;
- print one-second state and health snapshots;
- stop after a test duration or on Ctrl-C and join the worker before exit.

Use wired headphones to avoid feedback. Full commands and pass criteria are in the hardware test
protocol.

## Automated evidence

The Phase 2 test additions cover:

- valid, missing, and hash-mismatched model assets;
- exact dimensions, non-finite input, silence, deterministic repeated non-silent frames, runtime
  settings, reset, warm-up, and finite model output;
- deterministic CPAL format selection, DSD/layout rejection, fixed-buffer preference, all relevant
  sample conversions, and zero measured allocations in the conversion loop;
- compact stream issue classification and terminal/xrun accounting;
- construction, use, reset, and destruction of a deliberately non-`Send` reducer on its worker;
- stereo 44.1 kHz input through the 48 kHz mono model graph to stereo 96 kHz output;
- an exactly bounded worker command queue and cancellation-independent join;
- latency-aligned dry output and model reset counters after injected inference failures;
- legal/illegal lifecycle transitions, queue-duration validation, CLI parsing, and serializable
  inventory DTOs.

Repository gates for this phase are the standard formatting, warnings-denied Clippy, locked
all-target tests, license audit, offline model inference, frontend typecheck, and rustdoc checks.

The following checks passed locally on the primary macOS development host with Rust 1.98.0:

```text
cargo ci-fmt
cargo ci-clippy
cargo ci-test --locked
cargo deny check licenses
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo metadata --format-version 1 --locked --no-deps
cargo run --locked -p denoise-dfn --example offline_inference
cargo run --locked -p audio-engine --bin openkrisp-monitor -- --help
cargo run --locked -p audio-engine --bin openkrisp-monitor -- --list-devices
npm --prefix apps/desktop run typecheck
```

Inventory enumeration succeeded without opening an audio stream. Cross-platform compilation and
tests remain enforced by the Linux and macOS pull-request matrix. Windows support was subsequently removed.

## Local physical smoke-test evidence

On 2026-09-04, the release binary ran for 10 seconds from the built-in 48 kHz mono microphone to a
48 kHz stereo headphone output. The route reported:

- 942 input callbacks and 946 output callbacks with non-zero input/output levels;
- real DeepFilterNet inference, with observed p99 snapshots no higher than 4.115 ms against the
  10 ms model-frame deadline and zero missed deadlines;
- zero input overruns, discontinuities, model errors, resets, stream issues, terminal issues,
  xruns, and callback violations;
- 17 temporary output underruns, all handled with silence while the route remained `Running`;
- estimated fixed-plus-queued latency snapshots between 10.7 and 27.3 ms;
- a timed transition to clean process shutdown.

Two Bluetooth-duplex attempts used the same headset for capture and output. The first confirmed 45
real input callbacks, non-zero input level, and model inference before CoreAudio delivered a
terminal output-stream issue after the microphone activated. After adding compact failure-category
output, a three-second reproduction confirmed `StreamInvalidated` on the output after 44 input and
two output callbacks. CPAL emits that category when CoreAudio reports a nominal device sample-rate
change and requires the stream to be rebuilt. The Bluetooth profile transition remains an inference,
but the invalidated output configuration is directly observed. The supervisor detected each fault
and released the route; the succeeding built-in-microphone run reopened the headphone output
immediately.

## Known limitations and acceptance status

- Local capture and the short physical route are confirmed. Cleaned-audio audibility still needs a
  human listening result, and the 10-second observations are not a baseline latency measurement or
  a substitute for the two-hour soak.
- Phase 2 uses fixed nominal resampling ratios. Bounded rings prevent unlimited latency growth, but
  separate hardware clocks can drive a queue to an edge; adaptive occupancy feedback is Phase 3.
- Terminal stream faults fail and clean up the route. Automatic reconnect and recovery budgets are
  Phase 3 work.
- The adapter reuses project-owned arrays, but upstream DeepFilterNet internals are not asserted to
  be allocation-free per inference call. CPAL conversion and transport loops have separate
  allocation measurements.
- The deterministic fixture is a structural inference regression, not an audio-quality benchmark.
- The default model path targets the repository asset for the development CLI. A packaged desktop
  build must resolve the same validated asset through its application resource API; `--model`
  remains available for explicit headless runs.
- The executable is a local monitor only. System-wide capture and virtual endpoints remain the
  responsibility of future `platform-routing` implementations.

The Phase 2 software implementation is ready for the manual smoke and soak protocol. The roadmap's
complete definition of done should be marked accepted only after those results are recorded.
