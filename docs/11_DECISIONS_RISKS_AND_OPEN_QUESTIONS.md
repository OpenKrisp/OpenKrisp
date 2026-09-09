# Architecture Decisions, Risks, and Open Questions

## 1. Architecture decisions

### ADR-001: Tauri is the control plane only

**Decision:** No PCM audio crosses Tauri IPC.

**Reason:** WebView serialization, event delivery, and JavaScript scheduling are not suitable for a real-time audio path.

**Consequence:** All audio capture, processing, output, and routing stay in Rust.

### ADR-002: CPAL callbacks only move samples

**Decision:** Model inference and resampling run on a dedicated worker.

**Reason:** Callback execution must stay bounded and independent of model latency.

**Consequence:** The engine requires bounded rings and explicit underrun/overflow policies.

### ADR-003: Independent model histories per route and channel

**Decision:** Microphone and speaker routes do not share recurrent DeepFilterNet state. Phase 6 incoming stereo uses one mono instance per channel, all on the route DSP worker.

**Reason:** Each stream has an independent timeline and temporal model state.

**Consequence:** Concurrent routes increase CPU and memory usage predictably.

### ADR-004: Model construction occurs inside the worker

**Decision:** Create, use, reset, and destroy the concrete DeepFilterNet processor on the DSP worker thread.

**Reason:** Avoid relying on upstream inference state being `Send` or `Sync`.

**Consequence:** Startup requires a worker-readiness handshake.

### ADR-005: Canonical model rate

**Decision:** Process at the rate required by the pinned DeepFilterNet model, expected to be 48 kHz.

**Reason:** Model correctness takes priority over device-native format.

**Consequence:** Device routes require one or two resampling stages.

### ADR-006: Treat devices as separate clocks

**Decision:** Implement drift compensation based on output-buffer occupancy.

**Reason:** Nominally equal sample rates can still drift over long sessions.

**Consequence:** Output resampling must support gradual ratio adjustment or an equivalent clock-matching strategy.

### ADR-007: Latency-aligned bypass

**Decision:** Maintain a dry delay that matches the wet path.

**Reason:** Direct wet/dry switching changes timing and causes audible discontinuities.

**Consequence:** Bypass still incurs the processing graph's latency.

### ADR-008: Platform routing is isolated

**Decision:** Virtual microphones and system loopback live behind `PlatformAudioRouter`.

**Reason:** These mechanisms differ substantially across operating systems.

**Consequence:** Feature availability is reported at runtime and may differ by platform.

### ADR-009: Physical monitor before virtual drivers

**Decision:** The MVP is microphone to physical headphones.

**Reason:** It validates the hardest shared real-time engine work without installer/driver scope.

**Consequence:** System-wide usability comes in a later phase.

### ADR-010: Speech-first speaker processing

**Decision:** Speaker cleanup is explicitly speech-oriented and opt-in.

**Reason:** DeepFilterNet may alter music and general audio undesirably.

**Consequence:** Non-speech content bypasses by default unless a later classifier/gate is validated.

### ADR-011: Two bounded route classes with cooperative microphone priority

**Decision:** Permit one microphone and one incoming route. Incoming inference suspends under
microphone pressure and resumes after two quiet seconds; user bypass intent remains separate.
Native application sources are continuous bounded streams and fail visibly on output loss.

**Consequence:** No general graph scheduler, source mixing, EOF/draining player or OS-priority
promise is introduced. BlackHole 2ch is exclusive to a single virtual direction; Linux uses two
separate endpoint pairs. See the Phase 6 report for verification and hardware acceptance.

### ADR-012: Native submitted-render reference and estimated clock alignment

**Decision:** Tap output after CPAL device conversion and expose a separate raw capture companion,
using one bounded SPSC queue per native stream and a two-route owner-side reader registry. Keep
per-stream frame/epoch identity and map same-stream timestamp offsets onto process monotonic time.

**Reason:** A future AEC needs the signal submitted for playback, including silence and device
quantization. CPAL stream origins and physical sample clocks cannot be assumed synchronized.

**Consequence:** AEC consumers must honor closure, epochs and expiry, and resample/align their own
reference. Callback-anchored clock fits and device I/O delay estimates do not establish acoustic
or Bluetooth latency. No AEC or change to the existing drift controller is included. See
[the Phase 7 report](PHASE_7_RENDER_REFERENCE_IMPLEMENTATION_REPORT.md).

## 2. Risk register

| Risk | Impact | Likelihood | Mitigation |
|---|---|---:|---|
| DeepFilterNet API or model packaging differs from assumptions | High | Medium | Phase 0 dependency spike; pin revision; adapter boundary |
| Model misses real-time deadlines on some hardware | High | Medium | Measure p99; bounded queues; aligned bypass; supported-hardware floor |
| Input/output device clock drift | High | High | Adaptive resampling and occupancy controller |
| CPAL backend behavior differs across platforms | High | High | Backend isolation, hardware matrix, platform CI where possible |
| Virtual microphone requires signed native driver | High | High | Integrate existing endpoint first; separate driver project |
| System-loopback route feeds processed output back into itself | High | Medium | Graph validation, endpoint ancestry, emergency stop |
| Open-speaker echo remains after noise reduction | High | High | Recommend headphones; reserve AEC render-reference architecture |
| Bluetooth latency or profile switching breaks expectations | Medium | High | Diagnostics and explicit limitations; prioritize wired testing |
| Resampler allocates or has incompatible block constraints | High | Medium | Prototype in Phase 1; preallocate; wrap behind adapter |
| Queue overflow creates growing latency | High | Medium | Fixed capacity; drop-oldest/stale policy; metrics |
| UI event rate interferes with audio | Medium | Low | Atomic snapshots and throttled metrics |
| Model asset mismatch in packaged build | High | Low | Manifest/hash validation and packaging smoke tests |
| Worker panic leaves live CPAL streams | High | Low | Supervisor ownership, panic observation, deterministic teardown |
| Device identifiers are unstable | Medium | High | Persist preferences plus fallback heuristics, not names alone |
| Objective quality metric improves while sound worsens | Medium | Medium | Multiple metrics plus listening regression |

## 3. Open product questions

These do not block Phase 0 but should be resolved before their relevant phase.

### OQ-001: Primary operating system (resolved 2026-09-08)

OpenKrisp targets macOS and Linux only, subject to native hardware acceptance. Windows support,
its adapter, build integration and CI job have been removed. See [the removal decision](WINDOWS_SUPPORT.md).

### OQ-002: Frontend framework

React, Vue, Svelte, or plain TypeScript?

**Recommended default:** React + TypeScript + Vite for ecosystem familiarity. This does not affect the audio architecture.

### OQ-003: Virtual endpoint strategy

Require an existing third-party virtual audio device, bundle one, or build one?

**Recommended default:** Support one existing endpoint first. Treat a bundled driver as a separate project.

### OQ-004: Microphone channel policy

For multi-channel microphones, select one channel, mix down, or process multiple channels?

**Recommended default:** Explicit user-selected channel or documented mono mix for MVP. Never silently guess for professional interfaces.

### OQ-005: Speaker stereo policy

Process channels independently, downmix speech to mono, or use model-supported multi-channel behavior?

**Phase 6 decision:** The pinned asset is mono. Incoming stereo uses independent mono processors and preserves channel order; incoming multichannel and stereo-to-mono output are rejected. Production-model isolation tests pass; subjective stereo quality remains a hardware acceptance gate.

### OQ-006: Target supported hardware

What CPU tier and architecture must meet the p99 deadline?

**Recommended default:** Define one supported baseline per operating system after Phase 2 measurements.

### OQ-007: Target latency profiles

Should users choose Low, Balanced, and Stable profiles?

**Recommended default:** Internally support configurable targets, but expose one safe default until measurements justify presets.

### OQ-008: Background service

Should audio continue after all Tauri windows close or survive UI restart?

**Recommended default:** Tray-owned in-process engine for MVP; evaluate a sidecar/service after full routing is stable.

### OQ-009: Acoustic echo cancellation

Which AEC implementation and license are acceptable?

**Recommended default:** Defer implementation, preserve render-reference plumbing, and recommend headphones.

### OQ-010: Model distribution and license

Can the selected model and runtime be bundled under the application's intended distribution model?

**Required action:** Verify upstream licenses before release packaging.

## 4. Technical questions to verify in Phase 0

- Exact DeepFilterNet crate and feature names.
- Whether the low-latency model is bundled by a feature or loaded from an external asset.
- Exact model rate, frame size, channel layout, and latency.
- Whether model configuration exposes attenuation limit, post-filter, or other runtime controls.
- Whether the model processor is `Send` or `Sync` at the pinned revision.
- Whether output buffers can be reused without hidden per-frame allocations.
- CPAL loopback capabilities on each target backend.
- Device-ID stability and enumeration behavior.
- Best resampler for preallocated worker-thread use and dynamic ratio adjustment.

## 5. Change policy

When a verified fact contradicts this design:

1. Preserve the control/data-plane separation and callback safety rules.
2. Record the finding in `docs/ARCHITECTURE_VERIFICATION.md`.
3. Add or update an ADR for any architectural change.
4. Update affected interfaces and tests in the same change.
5. Avoid silently encoding an upstream quirk across multiple crates; isolate it in the relevant adapter.
