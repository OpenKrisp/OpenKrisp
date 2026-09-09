# Phase 7: Render reference and audio timing

## Result and scope

E16-01 and E16-02 add working native capture/render taps to the production routes and consumer-side
clock alignment/delay tracking. This is preparation for AEC. No echo canceller is selected, no AEC
runs before DeepFilterNet, and no open-speaker acceptance is claimed. The feature branch starts
from verified `main` / `origin/main` at `6a59ebb` (Phase 6 / PR #9).

## Reference placement and ownership

The output tap observes each CPAL buffer **after** the engine output ring, adaptive output
resampling, final limiter, channel mapping, safe underrun fill, and conversion to the negotiated
device sample type. It converts the submitted device samples back to normalized interleaved
`f32` for the reference. Thus integer quantization and actual underrun silence are represented;
queued worker output that never reaches the callback is never a render reference. Mono/stereo
channel order and output-device sample rate are retained. Wider integer and float64 formats are
represented at f32 precision. This is the application's submitted PCM, before OS mixing, output
volume, device effects, unreported transport buffering and acoustic playback.

Physical microphone and loopback inputs also expose a separate raw capture tap at CPAL conversion,
before the worker's input ring, downmix, resampling and model. Its metadata stays attached to its
PCM. It is an observation of raw capture; it does not retime the existing worker ring or claim
that all observed capture survived the worker's stale-input policy. A future AEC integration must
consume capture and reference and place cancellation before DeepFilterNet explicitly.

Each CPAL stream has its own producer, generation identity, controls and bounded queue. The
existing microphone/incoming route owners, DSP workers, model histories, sample rings, cooperative
priority, and occupancy-based drift correction remain independent. The new timing estimator does
not steer the existing resampler. There is no model operation on a callback and no new PCM IPC,
Tauri command, serializable audio type, event payload, dependency version or network service.
`audio-cpal` reuses the already locked `ringbuf` dependency.

## Native interface

- `AudioEngineHandle::audio_taps()` returns the engine's `NativeAudioTaps` registry.
- `NativeAudioTaps::take(&route_id, StreamDirection::Input | Output)` moves out the current
  generation's sole `AudioTapReader`. The registry holds exactly two route slots (microphone and
  incoming), with at most one pending capture and render reader each. Only owner/control threads
  lock this registry. Do not call `take` in a CPAL callback or an AEC processing loop.
- `HeadlessMonitorConfig::audio_taps` exposes the same registry to headless users; clone it before
  moving configuration into `HeadlessMonitor::start`. A registry belongs to one engine.
- An application PCM route registers a render reader; its application writer has no ADC timestamp
  and does not fabricate a capture clock. Use the physical microphone route's capture reader and
  the incoming route's render reader for future duplex work. Virtual microphone outputs are
  available as route observations but are not automatically selected as acoustic references.
- Readers may appear during startup and remain inactive until the route's PCM gate opens. Poll for
  readers outside the processing loop; after recovery closes an old reader, take its replacement.
  Repeated `take` returns `None` until another generation is registered. Taking a reader neither
  starts a route nor changes routing, defaults or processing mode.
- `read()` returns `Option<TimedAudioBlock>` using only bounded SPSC operations. `samples()` is
  interleaved PCM; `timestamp`, `frames`, and `discontinuity` describe the same block.
- Before using a retained block, check `reader.is_current(&block)`. This checks its stream/epoch,
  gates and age. Copied Rust data cannot be retracted; copying PCM into another unbounded history
  or ignoring invalidation violates the consumer contract. Data already handed to the native
  backend before Stop cannot be recalled.
- `reader.control()` provides atomic activity, closure, epoch, and tap `overflow_events()`.
  These counters describe observation loss, independently of the primary route's queue metrics.

A native consumer can feed `DuplexDelayTracker::observe(&reader, &block)` for both readers, then
call `estimate(MonotonicClock::default().now_ns())` and
`render_frame_for_capture(capture_frame, now_ns)`. Acquire the clock and readers outside the
processing loop. Drain with a fixed work budget and check each reader's closure/currentness before
using samples. The tracker checks the captured controls again when queried, so Stop or an xrun
revokes a cached estimate even without another block. The lower-level `AudioClockTracker` also
accepts metadata-only observations; its caller owns invalidation/reset responsibility.

## Timestamp contract

| Field/domain | Meaning |
| --- | --- |
| `stream_id` | Process-unique native stream generation; new on every construction/recovery attempt |
| `epoch` | Atomic continuity generation within that stream; changes on observation loss/timing faults/xruns |
| `frame_position` | First frame in this block, counted per channel from native stream construction, including gated callbacks |
| `callback_frame_position` | First frame in the original native buffer, before conversion/tap chunking |
| `sample_rate_hz`, `channels`, `direction` | Negotiated native format and capture/render direction |
| `observed_ns` | Original callback arrival, unsigned nanoseconds since one process-local `Instant` origin |
| `device_time_ns` | Optional CPAL capture/predicted playback time for this block's first frame, unsigned nanoseconds in this stream's opaque clock domain |
| `estimated_time_ns` | Optional capture/predicted playback time mapped to the process clock, signed nanoseconds |

The adapter initializes a shared monotonic origin on an owner thread before callbacks. The
callback copies that clock handle; it does not initialize a `OnceLock`. One monotonic clock read
per native callback anchors the mapping:

```text
estimated block time = callback arrival on process clock
                    + (CPAL capture or playback time - CPAL callback time)
                    + floor(block frame offset * 1e9 / native sample rate)
```

Only timestamps from the **same CPAL stream** are subtracted. Native stream origins can differ
arbitrarily. Capture may predate the process origin, hence signed estimates. Integer offsets are
computed relative to the original callback, including callbacks larger than the 2,048-frame
conversion scratch buffer; splitting again into 256-frame tap blocks does not accumulate rounding.

Zero, nonrepresentable, reversed-direction or greater-than-two-second CPAL delays produce `None`
for both times and advance the epoch. Native callback/audio timestamp regressions or stalls,
more than 100 ms disagreement between native callback progress and arrival progress, or more
than 20 ms disagreement between audio-time progress and the previous buffer's nominal frame
duration also invalidate the timing observation. A subsequent plausible callback can reanchor.
These conservative thresholds are software policy, not calibrated hardware confidence bounds.

`AudioClockTracker` fits at most 128 observations against the common process clock using bounded
least squares, with subtraction near the latest frame to limit floating-point precision loss.
Until at least 100 ms of frame progression is available, or when the fitted period is outside
±0.5% of nominal, it uses nominal rate and reports no rate-error estimate. The fit estimates
independent sample-clock rates, filters arrival jitter, and provides bounded fractional-frame
alignment. Timestamp history is bounded by observation count; it contains no PCM. Queries expire
200 ms after the latest arrival and extrapolate at most 200 ms beyond the retained frame window.

## Delay estimates and limitations

`DuplexDelayEstimate` reports the CPAL capture age and predicted render lead relative to their
respective callback arrivals. Their sum is `io_delay_ns`; a 0.1 EWMA reports
`smoothed_io_delay_ns`, updated only for a new callback pair. Chunk offsets are removed before
computing these components. Relative sample-rate error is available after both clock fits settle.
The tracker can map a microphone frame to the estimated contemporaneous fractional render frame;
it does not copy, interpolate, resample or acoustically correlate reference PCM.

This I/O delay estimate **excludes** the worker queues, model/frame/resampler delay, acoustic echo
path and unreported OS/device/Bluetooth delay. It is neither the existing `PipelineMetrics` route
latency estimate nor a measured end-to-end latency. Backend latency changes affect the estimate;
large discontinuities clear history instead of being smoothed as clock drift. Callback scheduling
bias, timestamp approximations, and small discontinuities remain sources of error. Availability
of CPAL timestamps does not prove their accuracy. Signal-based acoustic delay search, calibration,
AEC buffering requirements, resampling to AEC rate, and hardware uncertainty measurement remain
for subsequent E16 work. The fixed 200 ms reference window is a preparation limit, not an AEC
buffering requirement validated for Bluetooth or other high-latency paths.

## Buffering and discontinuities

Each tap allocates 64 fixed slots of at most 256 mono/stereo frames, and enforces a second cap of
`max(1, floor(sample_rate / 5))` queued frames (200 ms at normal audio rates). Tiny callbacks can
hit the slot cap first; at high rates the 16,384-frame slot cap can be tighter. Independently,
readers refuse blocks older than 200 ms by original callback arrival. This bounds stale retention
even if a producer stops calling back. No automatic queue expansion occurs. Unconsumed storage
remains bounded even when no AEC reader is attached.

| Event | Reference behavior |
| --- | --- |
| Startup prefill | Submitted prefill silence appears after the route gate opens; gated startup is not published |
| Output underrun | Exact zero-filled submitted PCM, at its predicted playback time; silence alone is not a clock discontinuity |
| Primary input/output ring drops or model reset | The raw capture and actual submitted render remain independently observed; no pre-render history is substituted |
| Tap overflow | Reject the remaining observation chunk, increment `overflow_events`, advance epoch; all older queued/retained reference is invalid; primary audio continues |
| Reader expiry/gap | Bounded stale discard; next returned discontinuous block is flagged; clock fit resets |
| Native xrun, realtime-denial or device-change notification | Advance epoch immediately, even without a following audio callback |
| Terminal stream error | Close the tap immediately in the error callback |
| Stop/Quit | Out-of-band cancellation gates both reading and publishing; owner closes both taps before device destruction/worker joins |
| Restart/device recovery | Fresh producers, queues, stream IDs and clock history; replacing a registry entry closes any previously taken reader too |
| Startup/recovery failure | Producer destruction closes the attempted generation; no successful route is fabricated |

Callback work adds fixed-size copies, integer timestamp arithmetic, one monotonic clock read and
atomic/SPSC operations. Allocation occurs at stream construction. Callback work scales with the
native buffer length and never waits for a reader, mutex or worker. Every reader call attempts at
most 64 pops. The existing worker wake and callback accounting are preserved.

## Validation

Final local macOS results (2026-09-09): **179 Rust tests passed**, zero failures, with the one
pre-existing opt-in timing benchmark ignored; **26 frontend tests passed**. This adds 19 Rust
regressions to Phase 6's 160-test baseline.

| Command | Result |
| --- | --- |
| `cargo ci-test --locked` | Passed: 179 tests; 1 pre-existing ignored benchmark |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo ci-fmt` | Passed |
| `npm run --prefix apps/desktop test` | Passed: 26 tests |
| `npm run --prefix apps/desktop build` | Passed: TypeScript and Vite production build |
| `npm run --prefix apps/desktop tauri -- build --no-bundle -- --locked` | Passed: optimized native executable with production frontend; launched successfully |
| `git diff --check` | Passed |

Local Rust logs are in `target/phase7-ci-test.log` and `target/phase7-clippy.log` (ignored build
artifacts). The existing worker stereo, model isolation, drift, overload, route lifecycle, routing
and IPC suites passed alongside the new tests. Deterministic new coverage:

- Submitted output reference equals f32 reconstruction of the actual f32/i16/u16 output, including
  quantization, stereo order, callback sizes 17–2,301 frames, partial underruns and silence.
- 44.1 kHz nested chunk offsets, raw native clocks with unrelated origins, unknown/regressing
  timestamps, device timeline gaps and recovery, and tap/stream format mismatch rejection.
- Slot/frame/age bounds at 8/44.1/48/96/192 kHz; overflow invalidates queued and retained blocks;
  Stop, producer drop, startup gates, errors and generation replacement cannot replay old PCM.
- Allocation counters observe zero allocations in tested output conversion/tap loops, including
  overflow and invalidation. This is software evidence, not a hardware callback deadline guarantee.
- Independent 48/44.1 kHz clocks at ±100 and ±750 ppm align within 0.01 frame in deterministic
  fixtures; jitter, rate steps, delay smoothing, chunk offsets, repeated polls, expiry and cached
  estimate revocation are tested.
- Registry tests cover sole-consumer transfer, independent microphone/incoming cancellation,
  recovery replacement and closing both old directions on a new route identity.

The actual Tauri app also passed a short [desktop smoke test](PHASE_7_DESKTOP_SMOKE_TEST.md):
built-in microphone → BlackHole Start/bypass/Stop/restart, native reference/timestamp inspection,
route-conflict controls, cancellation during startup, silent stereo incoming at 48 → 44.1 kHz,
and clean shutdown with a running microphone route. The optimized microphone run recorded one
missed processing deadline, with no observed overruns, underruns, stream issues or model errors.
This is functional evidence, not performance acceptance or a new listening-quality result.

Phase 6's two-hour concurrent hardware tests, physical device recovery, Bluetooth profile changes,
conferencing compatibility and broader listening remain **open**. The short 2026-09-09 BlackHole
→ AirPods Pro test remains only the Phase 6 listening/routing evidence already documented.
These results were collected locally; GitHub CI status is reported separately on the pull request.

## Verified dependency sources

The locked local CPAL 0.18.2 source and its
[StreamInstant contract](https://docs.rs/cpal/0.18.2/cpal/struct.StreamInstant.html) explicitly warn
that origins need not be shared across streams. Its
[output timestamp contract](https://docs.rs/cpal/0.18.2/cpal/struct.OutputStreamTimestamp.html)
defines playback as predicted. Source inspection of CoreAudio's `macos/device.rs` shows host time
plus/minus device buffer, safety and latency estimates. PipeWire's `stream.rs` uses graph time and
delay with fallback estimates; ALSA's `mod.rs` has creation-relative clock modes and timestamp
fallbacks. This implementation does not assume a hardware-quality marker that CPAL does not expose.

Next: E16-03, evaluate AEC implementations and licenses. Integration before DeepFilterNet and
open-speaker echo acceptance remain E16-04/E16-05.
