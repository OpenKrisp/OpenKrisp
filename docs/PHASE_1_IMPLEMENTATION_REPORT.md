# Phase 1 Implementation Report

## Scope

Roadmap Phase 1 implements the complete hardware-independent data-path foundation. It completes
Epics E1, E2, E4, and E5, plus the Phase 1 portions of E3 (`NoiseReducer` and its fake). It does not
create physical CPAL streams, a production DSP worker, or the production DeepFilterNet adapter.

## Implemented components

### Shared protocol

- Validated newtypes distinguish routes, physical devices, loopback/application endpoints, and
  virtual endpoints.
- `AudioFormat` uses non-zero rate, channel, and optional device-buffer units.
- Route configuration, processing mode, authoritative pipeline state, engine events, metrics, and
  sanitized errors are serializable without exposing PCM or backend handles.
- `ErrorCode` owns stable snake-case wire values instead of requiring clients to parse messages.
- Deserialization revalidates identifiers, and the DTO-safe graph has JSON round-trip tests.

### Pure DSP

- Checked interleaved views make sample-versus-frame and channel arithmetic explicit.
- Channel mapping supports pass-through, documented stereo averaging to mono, and mono duplication
  to stereo. Other mappings fail explicitly.
- `ModelFrameAssembler` accepts arbitrary complete-audio-frame chunks, emits exact model calls, and
  retains only the incomplete tail without shifting storage.
- `StreamingResampler` stages arbitrary chunks into preallocated blocks and writes through
  caller-owned output slices with explicit backpressure.
- Fixed dry delay, equal-power crossfade, non-finite-safe limiting, and peak/RMS meters complete the
  aligned wet/dry utility path.

### Reducer abstraction and fake

- `NoiseReducer` intentionally has no `Send` or `Sync` supertrait, preserving the verified
  DeepFilterNet worker-confinement constraint.
- Exact flat-planar dimensions and finite input/output are part of the contract.
- `FakeNoiseReducer` supports deterministic gain, runtime enable/bypass, reported statistics,
  configurable wall-clock processing delay, call-indexed/repeating failures, and reproducible reset.

### Bounded transport and simulation

- `ringbuf` 0.5.1 provides the lock-free SPSC storage behind frame-aware producer/consumer wrappers.
- Slice operations write and read complete interleaved frames only. Capacity never grows.
- A full producer drops newest excess input and records an overrun/discontinuity. The sole consumer
  can then discard oldest queued frames down to a target fill. Consumer-owned stale trimming avoids
  racing the SPSC read index, which `ringbuf` does not permit a concurrent producer to overwrite.
- Short output reads fill the remainder with silence and increment one underrun per callback.
- Fake source and sink callbacks use cyclic sizes and jitter patterns, exact rational clocks with
  independent ppm drift, bounded capture storage, and scheduled stream-error/device-loss injection.
- `SimulatedRoute` runs source and sink callbacks in timestamp order, moves audio only through the
  bounded rings, processes exact fake model frames, applies dry fallback on an injected reducer
  failure, and exposes start/run/stop state for deterministic tests.

## Dependency decisions

### Resampler

The selected adapter pins `rubato` 5.0.0 with default features disabled. It uses the asynchronous
sinc implementation in fixed-input mode because that implementation:

- exposes `process_into_buffer` for caller-owned output;
- reports input/output block bounds and group delay;
- resets preallocated filter history;
- supports gradual ratio adjustment when Phase 3 adds occupancy feedback.

The wrapper does not expose ratio adjustment yet. Phase 1 tests cover 44.1 to 48 kHz, 48 to 44.1
kHz, 96 to 48 kHz, and same-rate pass-through. One-second duration checks allow the small documented
streaming interpolation rounding difference rather than requiring cross-platform bit identity.

### Allocation measurement

`allocation-counter` 0.8.1 measures allocations made by the current test thread. After constructors
and one-time resampler dispatch, 100-iteration steady-state tests report zero allocations for:

- mapping, frame assembly, dry delay, crossfade, limiting, and metering;
- Rubato streaming `process_into_buffer` through the project adapter;
- bounded ring slice writes and reads;
- the complete deterministic fake-route loop.

These tests run in normal CI, but they do not prove behavior inside a future CPAL callback or the
current upstream DeepFilterNet inference call.

## Verification

Phase 1 adds deterministic coverage for:

- arbitrary/randomized mono and stereo chunk boundaries and sample order;
- resampling duration, constant-signal stability, finite output, and reset;
- exact dry delay, equal-power fade endpoints, controlled fade step size, and aligned bypass timing;
- limiter bounds/non-finite substitution and peak/RMS decay;
- ring capacity, ordering, high/low-water health, overrun, stale trim, and underrun silence;
- fake callback sizes, jitter, +100/-100 ppm clock math, wider drift stress, stream errors, device
  loss, reducer fallback, lifecycle, and queue bounds;
- serialization round trips and invalid identifier rejection.

## Known limitations and next work

- The Phase 1 route harness is mono and same-rate by design. It verifies scheduling and bounded
  transport rather than duplicating the production Phase 2 worker graph.
- Adaptive drift correction remains Phase 3 work; Phase 1 only produces deterministic independent
  clocks and observable queue effects.
- The production DeepFilterNet adapter still needs dimension guards, reusable conversion storage,
  settings application, reconstruction-based reset, non-finite fallback, and repeated-frame tests.
- Allocation measurements cover project DSP and fake infrastructure. Phase 0 established that the
  pinned upstream DeepFilterNet implementation is not currently proven allocation-free per call.
- Physical device, callback-thread, and long soak behavior cannot be verified until Phase 2.

The recommended next increment is the remaining E3 production DeepFilterNet adapter work, followed
by the CPAL inventory/format-negotiation portion of Phase 2.
