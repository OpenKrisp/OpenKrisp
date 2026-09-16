# Native AEC prototype — E16-04

Current desktop behavior is documented in [default enablement](AEC_DESKTOP_DEFAULT_IMPLEMENTATION.md).
The native-only/disabled-desktop descriptions below refer to the original prototype.

Implemented on `codex/aec-prototype`, based on PR #11 (`e794806`) and the E16-03
evaluation commit `187edf2`. This is an opt-in native prototype before DeepFilterNet,
not open-speaker acceptance. Desktop routes remain AEC-disabled.

Follow-up: [the E16-05 investigation](AEC_QUALITY_INVESTIGATION.md) fixes internal AEC3
stereo content detection, adds component-level quality gates and updates native allocation
and timing observations. The software measurements below describe the original E16-04
configuration. The subsequent [quality fixes](AEC_QUALITY_FIXES.md) resolve the three synthetic
failures and enable those gates in routine CI. Their report and manifest describe the current
native profile; the original measurements and ignored-test status below are historical.

## Implementation

`aec-webrtc` builds the selected WebRTC AEC3/APM revision through a small C ABI bridge.
The published sys source archive, Abseil 20240722.0 and its Meson definition are vendored
and SHA-256 checked. Meson 1.9.2 / Ninja 1.13.0 compile static libraries with downloads
disabled and an explicitly selected vendored Abseil. The build does not use the upstream
Rust wrapper, bindgen, a system APM, or system Abseil. Source provenance and reproduced
notices are in [third_party/aec](../third_party/aec/README.md).

The adapter accepts borrowed, fixed 480-sample f32 planes at 48 kHz. Capture is mono;
render is independently preserved mono or stereo. Multichannel render and high-pass
filtering are explicit. WebRTC noise suppression, gain controllers and amplification are
disabled. WebRTC estimates acoustic delay internally; `DuplexDelayEstimate.io_delay_ns`
is **not** passed as `stream_delay_ms`. APM's filterbank impulse peak is 430 samples
(8.958 ms) in the pinned configuration. This is a software fixture result, not measured
end-to-end or acoustic latency; frequency-dependent phase is not removed by a fixed delay.

All native construction, processing, reconstruction and destruction occur on the existing
microphone DSP worker, before its thread-confined DFN reducer. The adapter is `!Send` and
`!Sync`. Its FFI is isolated in `aec-webrtc`; `audio-engine` still forbids unsafe Rust.
Input validation and exceptions/native errors produce typed failures. A failed reset makes
the adapter unusable until a later successful reset. Callback code performs no AEC work.

Enable with `audio-engine`'s `aec-prototype` feature and
`HeadlessMonitorConfig.aec = Some(AecPrototypeConfig { reference_route_id, diagnostics })`.
The explicit ID must refer to the incoming route in the shared `NativeAudioTaps` registry.
Acquiring the microphone's own output as a reference is prohibited by slot ownership.
This covers only audio submitted by that incoming route, not arbitrary system playback.
The native harness requires four explicit device IDs and runs for 30 seconds:

```sh
python3 -m venv target/aec-tools
target/aec-tools/bin/pip install meson==1.9.2 ninja==1.13.0
cargo run --locked --release -p audio-engine --features aec-prototype --example aec_duplex -- \
  INCOMING_CAPTURE_ID PHYSICAL_RENDER_ID MICROPHONE_ID MICROPHONE_OUTPUT_ID
```

The four-device harness was compiled, not run against hardware. A subsequent
[native acoustic smoke test](AEC_LIVE_SMOKE_TEST.md) used an application PCM source,
built-in speakers/microphone and a BlackHole receiver through `aec_live`. It demonstrated
active AEC and signal transfer, including during user speech, with one combined processing
deadline miss. This is not open-speaker or conferencing acceptance. Keep headphone fallback.
The incoming capture bus and microphone output bus must be distinct. The current single
BlackHole 2ch integration does not establish concurrent macOS virtual-microphone and incoming
routing. This prototype does not change that restriction or confer conferencing compatibility.

## Timing and buffering contract

- **Primary capture:** when AEC is enabled, callback PCM and timestamps travel in the same
  `TimedAudioBlock` queue. The separately observed capture tap is not matched to a different
  sample ring. The existing sample-only path is retained for AEC-disabled routes. Native
  observers cannot take the AEC worker's sole capture reader. Metrics use actual timed-queue
  fill, high-water and overrun counters; the unused compatibility sample ring carries no PCM.
- **Placement:** render remains the PCM actually submitted at the CPAL output boundary,
  after DFN/bypass, drift correction, output resampling, mapping, limiting and device-format
  conversion. Submitted underrun zeros are reference audio; zero content is not a reset signal.
- **Units/domains:** frame positions belong to one stream generation/epoch; native device
  timestamps have unrelated origins. Signed estimated process-monotonic nanoseconds align
  the clocks. Original callback-arrival nanoseconds remain attached to copied reference
  samples. Wall-clock time is not used. Unknown or expired timing is unavailable, not zero delay.
- **Capture grid:** the native capture origin, canonical emitted-sample count and input
  resampler's output delay map each 480-sample microphone frame back to its native capture
  time. DFN receives exactly the resulting AEC output, including during denoise bypass.
- **Reference conversion:** two bounded 128-point clock fits require at least 100 ms of
  observations. Relative render/capture rate error must be within ±1000 ppm. A 32-tap,
  128-phase windowed-sinc interpolator maps native render samples once onto the capture's
  48 kHz grid. Both channels use identical positions without downmixing. Native rates are
  restricted to 8–96 kHz for this prototype. Interpolated samples are bounded to [-1, 1].
- **Queue limits:** callback taps retain 64 slots of at most 256 native frames and at most
  200 ms of frames. Primary capture has a stricter configured cap (100 ms by default,
  stale target 80 ms). AEC rejects capture capacity above 200 ms. Original callback age
  must remain ≤200 ms both on read and on every contributing history sample.
- **Copied history:** 32,768 native frames, each with two f32 values and a u64 arrival time,
  use 524,288 bytes plus 16,384 bytes of sinc coefficients and fixed metadata. Oldest slots
  overwrite in place; age checks can make samples unusable before physical overwrite.
  This deliberately replaces E16-03's proposed 20 canonical blocks with native-indexed
  storage, allowing one clock-compensating resampling stage. It is a storage bound, not
  added playback latency. Insufficient history causes immediate aligned fallback.
- **Cadence:** at most 64 reader calls, one 10 ms reverse-analysis call and one 10 ms capture
  call per microphone frame. No reference-wait queue, mutex registry access, or catch-up
  inference loop is added to the worker. Native APM has additional configuration-sized
  histories and an upstream 100-frame render transfer capacity; this is not an application
  queue or permission to accumulate a second of latency.

Normal callback paths remain bounded and nonblocking. They copy PCM, update atomics and
wake the existing worker; no inference, new allocation, file I/O, normal logging, or mutex
wait was added. Native APM itself allocates and uses internal synchronization on its worker.

## Lifecycle and fallback

The owner polls for a reference, takes only the associated incoming generation and transfers
its reader through a capacity-one channel. The worker never takes the registry mutex.
Incoming Stop/close, epoch change, invalid timing, expiry or an analysis failure revoke learned
AEC state before another valid capture frame can reuse it. Device recovery requests a fresh
reader from the same explicit route. A change in render rate/channel count reconstructs APM.
The microphone does not stop or rebuild the incoming route.

Microphone Stop closes its streams and joins its worker. The worker returns its reference reader
(including an unconsumed handoff) through a capacity-one channel. The owner restores it only
if its generation is still current, so microphone restart can reuse the active incoming stream
without overwriting a recovered generation. Primary capture replacement also revokes old
observer readers. Cancellation gates continue to prevent output after Stop.

Capture trim/overflow/xrun and output discontinuities reset capture resampling, partial model
frames, timing, AEC, DFN and delayed microphone history. Audio captured during a reset is
invalidated. Fresh startup and the first post-reset block do not reset DFN a second time.
Tests exercise this through the actual timed worker transport using the `audio-cpal/test-support`
synthetic backend, which is absent from default application builds.

AEC fallback emits one mono frame through its own 430-sample microphone delay. Missing reference
never pauses capture. Recovery blends into AEC over 20 ms with a linear mix; invalidation uses
the current delayed microphone immediately and never blends cached invalid AEC output. That
transition can be audible and is not a listening acceptance claim. Existing DFN delay/bypass,
output limiter, stereo incoming processing, output drift control and route ownership remain.

AEC status/counters are native-only (`AecDiagnostics`): waiting, aligning, running, invalid
timing, native error, overload and stopped are distinct. Counts include AEC/fallback frames,
resets, errors and processing deadline misses. Combined AEC + DFN processing cost participates
in the existing inference deadline/overload guard. Native failures or AEC-only deadline overruns
use a bounded 100-frame retry cooldown. Existing metrics still distinguish estimated latency
from a measurement; AEC's fixed software delay is included in the estimate.

## Measured software results

Deterministic native fixtures passed on macOS ARM64 and Linux x86_64 (Debian container on
the macOS host). Both are native target builds; Linux execution used the container engine's
x86_64 support and is not a Linux hardware latency measurement.

| Fixture | Result |
| --- | --- |
| Independent stereo linear echo, delays 40/100/160 ms; last 4 s of 9 s | 18.06 / 18.06 / 18.05 dB ERLE; >10 dB regression floor |
| Anti-phase stereo far-end signal | 20.70 dB ERLE; channels do not cancel during mapping |
| Modulated harmonic near-end with simultaneous echo | Near-end projection gain 0.824; correlation 0.918 macOS / 0.917 Linux against the same APM processing near-end alone; targets ≥0.7 / >0.8 |
| Silence, malformed/NaN frames, reset versus fresh state | Passed; fixed output duration and finite results |
| Pinned APM impulse peak | 430 samples on both targets |
| Rust allocations in native render/capture calls | Zero in the deterministic far-end fixture |
| Clock/rate conversion | 44.1/48/96 kHz capture/render combinations at ±100/±750 ppm; channel order, drift rejection and original-age expiry passed |

**Original failed acceptance fixture:** stationary broadband-noise double talk had gain **0.368**
and correlation **0.734**, below the same 0.7 / 0.8 targets. The initial impulse-aligned raw
comparison was also inadequate because of filterbank phase; the reported result uses a
separate near-end-only APM baseline. The target was not lowered to pass this fixture.
At this revision, `broadband_double_talk_acceptance` was explicitly ignored in routine CI and
failed when run with `--ignored`. The [later quality fixes](AEC_QUALITY_FIXES.md) resolve this
fixture without lowering its targets. Room acoustics and listening acceptance remain E16-05
work. The engineering prototype is available only through explicit native opt-in.

```sh
# Run the current regression (the original negative result requires the original revision):
cargo test --locked -p aec-webrtc broadband_double_talk_acceptance -- --nocapture
```

Standalone C++ `new`/`new[]` instrumentation (including aligned forms) found **4,151** calls
requesting approximately **1.61 MB** during construction, then **665** calls requesting
approximately **645 KB cumulatively** over 1,000 measured frames after 100 warm-up frames.
These are allocation traffic, not live or peak resident memory. Direct C allocation functions
are not intercepted. The implementation does **not** claim an allocation-free native worker
or a complete native heap measurement. See `scripts/probe-aec-native.py`.

| Timing diagnostic | p50 / p95 / p99 / max (µs) | Misses (>10 ms) |
| --- | --- | --- |
| AEC-only C++ probe, macOS | 193 / 237 / 252 / 293 | 0 / 1000 |
| AEC-only C++ probe, Linux container | 59 / 75 / 90 / 737 | 0 / 1000 |
| Full software graph with real DFN, release macOS | 1030 / 1377 / 1642 / 1804 | 0 / 1000 |
| Same graph with two bounded CPU load threads | 970 / 1053 / 1068 / 1129 | 0 / 1000 |

The graph benchmark verified 1,090 active AEC frames and 10 initial alignment-fallback frames,
with no native/model errors or AEC resets in both runs. It uses synthetic timestamps and
drains output without a device, so queue occupancy, device underruns and the saturated
occupancy drift correction are not hardware acceptance results. Runs are short and subject
to host scheduling; the load run being faster does not establish a performance improvement.
Construction/reconstruction in the C++ probe were 521/171 µs on macOS and 6515/531 µs in the
Linux container. Construction and resets stay outside callbacks.

## Validation and remaining work

Final check totals and commands are recorded in [the evidence manifest](evidence/aec-prototype-2026-09-10.json).
The required Rust workspace tests, Clippy, formatting, frontend tests/build and diff check
are run for this change; native build integrity/extraction tests and the release benchmarks
are additional checks. Performance tests are opt-in, not brittle shared-runner CI thresholds.
The desktop production frontend is unchanged. Subsequent physical-device observations and
their limitations are recorded in [the acoustic smoke test](AEC_LIVE_SMOKE_TEST.md) and its
separate evidence manifest; the original software evidence manifest remains historical.

E16-04 delivers the prototype. E16-05 open-speaker/quality acceptance and E16-06 documented
headphone-fallback acceptance remain open, as do production notice review, full native heap
measurement, broader hardware/rate/CPU matrices and shipping AEC controls. The short physical
smoke test does not establish processed-voice listening, Bluetooth-profile, device-recovery
or conferencing acceptance.
Phase 6's two-hour concurrent hardware tests and all previously open hardware gates remain
open; the earlier short BlackHole → AirPods listening test does not close them.
