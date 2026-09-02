# Test Strategy

## 1. Objectives

The test strategy must prove more than audio quality. It must also prove timing, bounded memory, correct lifecycle ownership, format correctness, and recovery from failures.

The engine should be testable in CI without a microphone, speaker, or graphical desktop.

## 2. Test layers

| Layer | Purpose |
|---|---|
| Unit tests | Verify pure DSP and domain behavior |
| Model adapter tests | Verify DeepFilterNet loading, dimensions, state, and fallback |
| Deterministic simulation | Verify callbacks, clock drift, queue policies, and lifecycle |
| CPAL integration tests | Verify actual backend/device behavior |
| Tauri tests | Verify commands, DTOs, events, persistence, and UI state |
| Audio quality regression | Detect model/configuration regressions |
| Soak and stress tests | Detect leaks, deadlocks, deadline misses, and unbounded latency |
| Platform routing tests | Verify virtual endpoints, loopback, permissions, and loop prevention |

## 3. Fake audio backend

Implement a deterministic backend capable of producing and consuming timestamped frames.

Required controls:

- Sample rates: at least 44.1, 48, and 96 kHz.
- Channels: mono and stereo.
- Sample formats: logical `f32` plus conversion tests for integer device types.
- Callback sizes: fixed and randomized.
- Input and output clocks with configurable drift in ppm.
- Callback jitter and delayed callbacks.
- Device disconnect and reconnect.
- Stream errors.
- Sink underruns and source overruns.
- Controlled model processing spikes.

Example callback sizes:

```text
64, 96, 128, 240, 256, 480, 512, 960, 1024 frames
```

## 4. Unit tests

### Frame assembler

- One exact model frame.
- Multiple model frames in one input chunk.
- One model frame split across many chunks.
- Random chunk sequence with exact reconstructed output.
- Stereo channel alignment.
- Reset discards pending partial data.
- No out-of-bounds writes.

### Channel mapper

- Mono to mono.
- Stereo to mono using the documented policy.
- Mono to stereo duplication.
- Stereo pass-through.
- Invalid channel counts rejected.

### Resampler

- Duration preserved within expected rounding.
- Constant signal remains stable.
- Impulse response has finite output and documented delay.
- 44.1 -> 48 kHz.
- 48 -> 44.1 kHz.
- 96 -> 48 kHz.
- Reset after discontinuity.
- Adaptive ratio updates remain within bounds.

### Delay and crossfade

- Dry delay equals configured sample count.
- Bypass transition does not change output length.
- Fade begins and ends at expected gains.
- No discontinuity above a chosen test threshold for a controlled waveform.

### Limiter and meters

- No output beyond configured safety range.
- NaN and infinity handling.
- Peak and RMS values for known signals.
- Meter reset and decay behavior.

## 5. DeepFilterNet tests

- Packaged model manifest resolves correctly.
- Hash mismatch is rejected.
- Silence produces finite output.
- Known noisy fixture produces finite output of matching duration.
- Incorrect model-frame dimensions return typed errors.
- Repeated processing does not panic.
- Reset works after a discontinuity.
- Runtime settings either apply or return `RequiresRestart`.
- One injected inference failure produces aligned fallback.
- Sustained failures transition the worker to degraded mode.

Do not make subjective listening the only quality gate.

## 6. Deterministic pipeline tests

### Timing and queue tests

- Arbitrary input callback sizes produce correct model frames.
- Arbitrary output callback sizes consume output correctly.
- Startup prefill avoids an immediate underrun under expected timing.
- Input overflow drops stale data and keeps latency bounded.
- Output underrun writes safe fallback and continues.
- Queue capacity never grows.

### Clock-drift tests

Simulate at least:

- Input +100 ppm relative to output.
- Input -100 ppm relative to output.
- A wider stress range chosen for robustness testing.
- Clock rate step after a simulated device change.

Pass conditions:

- Output fill converges near target.
- Correction stays bounded.
- No periodic hard sample drops in normal drift handling.
- Estimated latency remains bounded.

### Lifecycle tests

- Start -> Running -> Stop -> Stopped.
- Stop during Starting.
- Duplicate Start rejected.
- Start after Failed.
- Device disconnect -> Recovering -> Running.
- Recovery budget exhausted -> Failed.
- Shutdown with active route.
- Worker panic observed and cleaned up.

## 7. Allocation and blocking checks

Use appropriate tooling or instrumented allocators in dedicated tests to inspect steady-state loops.

Targets:

- No expected heap allocation in CPAL callback bodies after startup.
- No callback waits on a blocking synchronization primitive.
- No unbounded channel or queue.
- No per-frame creation of model tensors when reusable views are possible.

The exact tooling can vary by platform, but the test report must document what was measured and what remains inferred.

## 8. Audio quality regression

Maintain licensed or project-owned fixtures:

```text
clean speech
speech + stationary fan noise
speech + keyboard noise
speech + street noise
speech + competing background speech
non-speech music fixture for bypass validation
```

Possible objective summaries:

- Signal level and clipping.
- Output duration.
- SNR improvement on synthetic mixtures.
- SI-SDR or another suitable metric on controlled fixtures.
- Spectral difference and silence residual energy.
- Model-provided attenuation or speech metrics when available.

Do not optimize one objective metric without listening tests. Some perceptual metrics have licensing or usage constraints; verify before adding them to CI.

## 9. Soak and stress tests

### Two-hour baseline soak

- One physical or simulated microphone route.
- Metrics sampled periodically.
- Assert no steadily growing memory or queue fill.
- Assert no deadlock or lost worker.
- Record underruns, overruns, inference p99, and drift correction.

### CPU-pressure test

- Add controlled background CPU load.
- Inject model execution spikes.
- Verify degraded/bypass behavior.
- Verify callbacks remain responsive.

### Reconfiguration test

- Switch devices repeatedly.
- Toggle bypass repeatedly.
- Change supported settings during processing.
- Sleep/resume where automation allows.

## 10. CPAL hardware matrix

For each supported operating system, test when available:

- Built-in microphone and output.
- USB microphone.
- USB audio interface.
- Separate input and output devices.
- Wired headphones.
- Bluetooth headset, with limitations recorded.
- Default-device change during a route.
- Device removal during a route.

Record negotiated formats because behavior can differ by host and device.

## 11. Platform-routing tests

- Virtual endpoint discovery.
- Missing endpoint instructions.
- Start and release graph connections.
- Application restart with endpoint still installed.
- Feedback-loop rejection.
- System-loopback source does not capture the processed destination recursively.
- Permission denied and permission later granted.
- Installer/uninstaller cleanup where custom components exist.

## 12. Tauri tests

- DTO validation.
- Stable error codes.
- Bounded command queue full behavior.
- Snapshot reconciliation after frontend reload.
- Metrics event throttling.
- Configuration migration.
- Quit with active route.
- Tray mode behavior.
- No command exposes arbitrary file or shell execution.

## 13. CI gates

Every pull request:

```text
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
frontend typecheck
frontend unit tests
```

Scheduled or release CI:

- DeepFilterNet fixture suite.
- Deterministic drift simulations.
- Long soak tests.
- Platform-specific CPAL tests on available runners.
- Tauri packaging smoke test.
- Model manifest/hash validation.

## 14. Release acceptance checklist

- No known callback blocking path.
- No intentionally unbounded audio queue.
- p99 inference within the supported-hardware budget.
- No sustained queue drift in the required scenarios.
- Bypass is latency-aligned and click-resistant.
- Device loss is recoverable or clearly reported.
- Shutdown releases streams and joins workers.
- Model asset is pinned and integrity-checked.
- Platform limitations are visible in the UI and documentation.
