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

## 15. Implemented suite and execution tiers

The deterministic `SimulatedRoute` now drives `worker/graph.rs`, the same processing core used by the DSP thread. It includes production channel mapping, both resamplers, frame assembly, queues, drift correction, overload policy, and discontinuity resets. Its clock injects processing cost without sleeping. The callback matrix covers all pairs of 44.1/48/96 kHz and mono/stereo with irregular blocks and jitter. The long drift tests measure several minutes of simulated time and average correction after settling. They allow an initial underrun for large irregular blocks, then require continuous output; the fixed 10 ms drift scenarios require zero underruns throughout.

`headless/backend.rs` injects generation creation on the route owner, outside callbacks. The real monitor supervision and teardown run against fake owned streams and real worker threads. Tests record each stream drop, tap closure and reducer drop, and exercise partial creation failures, replacement, exhausted retries, cancellation, and worker panic. Native format negotiation and CPAL activation still require platform/hardware tests.

| Tier | Command / CI location | Scope |
|---|---|---|
| Fast | `cargo ci-test --locked`; `npm test --prefix apps/desktop`; `python3 -m unittest discover -s scripts/tests -v` | DSP/domain units, production graph matrix, recovery, IPC contracts, updater lifecycle, frontend consumers, installer/release guards |
| Feature coverage | `cargo test --locked -p audio-engine --all-targets --no-default-features` | Exercises the engine without AEC, in addition to all-feature tests |
| Extended | `python3 scripts/run-extended-tests.py extended` | Release-mode drift, real speech quality, AEC → DFN quality, and production reset recovery; nightly and release jobs |
| Native desktop | Build with `--features desktop-smoke`, then `python3 scripts/test-desktop-native.py BUNDLED_BINARY` | Real WebView IPC and rendered UI state, close/hide, show, reload, quit, private settings flush, bundled-model hash, and ordinary engine startup; macOS and Linux/Xvfb PR jobs |
| Soak | `python3 scripts/run-extended-tests.py soak --seconds 7200` | Two real DSP threads with production microphone and stereo DFN models, synthetic paced callbacks, pressure/bypass changes, bounded queues/latency, RSS sampling, clean joins; nightly |
| Reference performance | `python3 scripts/run-extended-tests.py performance --profile apple-m3-max` | Paced production AEC + DFN graph; requires the exact CPU/OS/architecture profile |

Long tests are explicitly ignored in the fast inventory. The extended runner verifies exact test names before executing them and rejects a zero-test success. Logs and JSON measurements are written under `target/test-evidence/`; CI uploads them even on failure. A short `--seconds 35` soak verifies the runner, pressure and bypass transitions, RSS collection, and joins. It does **not** establish two-hour stability.

The native smoke build has no fixture IPC commands and uses the production invoke handler, window-close handler, and tray dispatch. `--smoke-fixture` supplies two control-plane routes with private settings; `--smoke-startup` uses the ordinary `AudioEngine::start` path without opening an audio route. It dispatches the shared tray handler programmatically, so OS menu clicking, microphone permissions, physical capture/render and unplug/resume acceptance remain separate. Fixture code and switches are absent unless `desktop-smoke` is enabled. Release packaging does not enable it.

### Shared desktop contract

`tests/fixtures/desktop-contract.ts` is one JSON-compatible literal used by Rust and TypeScript. Rust serializes current types and checks real command-handler responses against it; TypeScript checks its types, command envelopes, subscriptions, state reconciliation, and rendered updater statuses. It includes non-null formats, devices, model metadata, AEC diagnostics, stream issues, routing endpoints, faults, and all current pipeline/update states.

Regenerate only when intentionally changing the contract, then review the fixture diff and run both consumers:

```sh
OPENKRISP_UPDATE_CONTRACT=1 cargo test --locked -p openkrisp-desktop --lib shared_desktop_contract
npm run --prefix apps/desktop typecheck
npm test --prefix apps/desktop -- --run src/contract.test.tsx
```

Enabled updater tests use the same private preparation/installation orchestration as production. A bounded localhost server exercises the pinned updater plugin's actual HTTP download and minisign verification with a public test vector. Tests cover overlapping checks, transport errors, progress, tampered bytes, late completion after shutdown, cancelled installation with retained bytes, concurrent install/quit, and settings flush before restart. No platform installer runs in these tests. Native tests also guard the default bundle's required disabled updater configuration.

### Speech regression corpus and budgets

`tests/fixtures/speech/manifest.json` records two LibriSpeech dev-clean clips, original archive paths, FLAC and PCM checksums, attribution, conversion, and CC BY 4.0 licensing. Tests run offline. Fan, keyboard, and street noise are deterministic generated mixtures; they are representative regression signals, not recordings of those environments. Competing speech uses the second clip. `scripts/tests/test_test_tiers.py` verifies fixture integrity.

Scores compare aligned references at the declared 480-sample DFN delay. Clean speech must keep gain within 0.5–1.5, correlation above 0.8 and SI-SDR above 5 dB. Generated noise mixtures require at least 0.5 dB SI-SDR improvement and gain within 0.4–1.6. Competing speech checks preservation and less than 1 dB SI-SDR loss; DFN does not provide speaker isolation. Real stereo mixtures must match two independent mono histories, improve both channels, reject swapped channels, and clear both histories on reset. Mutation controls reject untouched noise, muted speech, wrong frame delay, and substituted speech.

The production AEC → DFN test compares real double-talk speech against the paired full-chain reference, checks finite limited final output and actual native AEC frame counts, and checks echo-only suppression and absolute speech RMS gain within 0.5–1.5. Its real-speech regression floor is 3 dB ERLE; the initial local result was approximately 3.9 dB. Revoking the render reference must fail that gate. This is a modest regression floor, not a claim that the existing 10 dB synthetic acceptance target has been met on a representative real-speech corpus. More speakers, languages, noise recordings, subjective listening, and hardware echo paths remain required for broader acceptance.

### Performance profiles

`tests/performance/profiles.json` identifies the reference machine and conditions. The initial Apple M3 Max profile uses paced 10 ms frames, no CPU burners, at least 1,000 measured frames and 1,000 actual AEC frames, zero model errors, no skipped DFN inference or degraded bypass, p99 ≤ 8 ms, and deadline misses ≤ 1%. The runner rejects unrecognized hardware, malformed reports, wrong conditions, fallback-only measurements, and missing tests. Add other profiles after measurement; do not apply this profile to arbitrary hosted Linux/macOS machines.

The optional manual performance job requires a self-hosted runner labelled `openkrisp-perf` and `apple-m3-max`; runner provisioning is separate. Hosted release timing checks remain diagnostics. These software graph measurements do not measure end-to-end hardware latency or satisfy the physical-device acceptance matrix. E10-05/E10-06 and E17 hardware acceptance items remain open until the corresponding long-run/platform evidence is recorded.

### Local validation of this implementation

On Apple M3 Max / macOS / arm64: the fast workspace inventory passed (233 tests; seven explicit long checks ignored), the no-AEC inventory passed (83 tests; four ignored), TypeScript typecheck and 99 frontend tests passed, and all 78 Python tests passed with native compilation access. Formatting, all-feature and no-AEC Clippy, and workflow actionlint passed. The extended release inventory, both packaged native smoke modes, and the reference performance gate passed. The latest measured graph p99 was 833 µs with zero deadline misses over 1,000 measured frames. A 35-second concurrent soak measured 416,016 → 416,192 KiB RSS and a maximum estimated queue/processing latency of 52.8 ms, and joined both workers.

The full two-hour scheduled soak, Linux native smoke, and physical device/permission/unplug/resume checks have not been executed locally. CI workflow changes still require execution on their target runners. This evidence does not close those acceptance items.
