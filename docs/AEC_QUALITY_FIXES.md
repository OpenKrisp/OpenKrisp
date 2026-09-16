# AEC stereo delay and speech-preservation fixes

Subsequent change: [desktop default enablement](AEC_DESKTOP_DEFAULT_IMPLEMENTATION.md)
was explicitly requested by the user. The disabled-desktop status below describes
this software-quality revision; its measurements remain historical and unchanged.

This follow-up fixes the three previously failing synthetic AEC quality gates and
enables them in routine CI. It also adds early local-speech, varied speech,
reflection, nonlinear-speaker and unequal-coupling controls. **E16-05/E16-06 remain
open:** these are software results, not physical-room or conferencing acceptance.
Desktop AEC remains disabled. No microphone recording or physical audio-flow test was run
for this software report; read-only device enumeration confirmed the Mac's test devices.

## Stereo delay reference

The earlier diagnostic found an old delay estimate persisting after a 100 → 160 ms
path change. AEC3's default delay matcher selects one render channel by playback
energy. That channel need not dominate the echo at the microphone. Swapping the
left/right acoustic gains or adding reflections exposed additional recovery failures.

The prototype now uses both channels for the **delay-estimation sidechain**. Its
stereo average falls back to the stronger original channel when destructive
interference removes more than half the power expected for uncorrelated channels:
`power(average) < 0.125 * (power(left) + power(right))`. Opposite-phase playback thus
remains observable. Silence yields silence; there is no retained sample history.

The echo filters still receive the two original render planes in their original
order. The application tap still contains exactly the PCM submitted to the output.
The sidechain changes neither speaker playback nor the reference available to other
native consumers. The helper processes one existing 64-sample low-band block at a
time, using fixed loops and stack scalars, without allocation or I/O.

`scripts/build-aec.py` verifies/extracts the pinned archive, then adds an include and
one helper call to the native `AlignmentMixer` downmix branch. Each original call
site must match exactly once; an unexpected upstream revision fails the build
without partially editing the file. The helper lives in
`crates/aec-webrtc/src/render_alignment.h`. Original source archives and upstream
copyright/license notices remain intact. This is a maintained native source change,
not an unmodified-upstream behavior claim.

The stereo candidate-error fraction is 0.6, while mono retains 0.2. The fraction
compares matched-filter residual error with capture energy; it is not a confidence
percentage or an acceptance score. A single speaker and differing echo responses
can leave substantial error when matched against the stereo average. Upstream
excitation checks, lag voting and delay hysteresis remain. Trials with stricter
matching still failed recovery; accepting a candidate also requires the unrelated-
render/local-speech controls to pass.

## Speech preservation

The [earlier investigation](AEC_QUALITY_INVESTIGATION.md) isolated most broadband
local-signal loss after the linear filter. The production prototype now shares a
single native profile with the diagnostic through `src/config.h`:

| Parameter | Earlier explicit-channel profile | Current prototype |
| --- | --- | --- |
| Linear ERLE estimator upper bounds, low/high frequency power ratios | 4 / 1.5 | 16 / 16 |
| Near-end high-frequency transparent/suppress/masking ratios | 0.1 / 0.3 / 0.3 | 1.09 / 1.29 / 0.3 |
| Dominant near-end echo/near-end threshold | 0.25 | 2 |
| Dominant near-end trigger, 4 ms blocks | 12 | 3 |
| Default acoustic gain amplitude when no usable linear estimate exists | 1 | 0.05 |

Raising the ERLE ceiling allows the estimator to use measured filter convergence;
it does not assign that convergence to every frame. More responsive local-speech
detection and near-end masking preserve short unvoiced segments. Normal suppressor
masking thresholds, saturation handling, WebRTC NS/AGC-off settings and APM high-pass
filtering remain unchanged.

The default-gain change addresses a separate failure: unrelated playback could
nearly mute early local speech even though there was no echo in the microphone.
It reduces the assumed acoustic coupling until a usable linear estimate exists.
It is an assumption, not a measured room gain. This tradeoff needs physical startup
and path-change listening; software checks explicitly cover both early local speech
and early echo, so merely passing voice through cannot satisfy the suite.

PCM remains native. Configuration, inference, reset and destruction stay on the
owning microphone worker. No callback, route association, clock fitting, reference
expiry, fallback, DFN ordering or Tauri interface changed. The impulse-peak delay
remains 430 samples; that value is not measured end-to-end acoustic latency.

## Regression coverage

All existing acceptance thresholds and original scoring windows remain:
echo-only ERLE >10 dB, local projection gain in `[0.7, 1.5)`, correlation >0.8.
Voiced/unvoiced segments remain separate, with a paired same-stage local-only
baseline. The three original gates no longer carry `#[ignore]`.

Additional controls include:

- Unrelated playback with local speech at two amplitudes, two noise seeds, and
  mono/independent-stereo/opposite-phase layouts, scored at seconds 1–3 and 7–10.
- Two additional pitch/seed combinations at quieter/normal local amplitudes using
  the same component scores. These broaden synthetic coverage, not human speech
  intelligibility or independent statistical validation.
- Twenty reflection/distortion/coupling cases: two noise seeds, five left/right
  gain pairs (including left-only and right-only acoustic coupling), and linear or
  smoothly saturated speakers. Delay changes 100 → 160 → 40 ms. The original
  two-second recovery allowance remains; early echo is also scored at seconds 1–3.
  The first second and real room transients still need physical validation.
- Seven native sidechain contracts covering duplicated mono, opposite phase,
  unequal opposite phase, either silent channel, independent stereo and silence
  after signal. They also verify the original render planes remain unchanged.

`python3 scripts/check-aec-quality.py` checks the required inventory, includes any
ignored tests, runs the compiled native mixer contracts, and retains logs/hashes.
CI also runs the native mixer contracts directly. Build failures, skipped required
tests and incomplete runs cannot report quality success.

## Validation and limitations

The final measurements, commands, source hashes and local log hashes are recorded in
[the evidence manifest](evidence/aec-quality-fixes-2026-09-15.json).

| Original regression | Earlier macOS result | Current macOS result |
| --- | --- | --- |
| Broadband local preservation, gain / correlation | 0.380 / 0.742 | 0.984 / 0.956 |
| Quiet independent-stereo voiced preservation | 0.578 / 0.811 | 0.942 / 0.960 |
| Quiet independent-stereo unvoiced preservation | 0.265 / 0.612 | 0.710 / 0.822 |
| Stereo combined delay/gain recovery, seconds 8–10 ERLE | 6.997 dB | 21.346 dB |

The complete native quality runner passes **15 Rust tests, none ignored**, plus
**seven compiled alignment contracts** on macOS ARM64 and Linux x86_64. Linux used
the same runtime profile and the expanded speech/reflection/distortion suite. Twenty
extra seconds-1–3 echo windows were added after the disposable Linux run and passed
on macOS; the Linux result covers the other sixty windows of that twenty-case fixture.
The Linux runner's source hashes retain that distinction. Platform floating-point
differences are visible: the quiet independent-stereo unvoiced gain/correlation is
0.795 / 0.848 on Linux. This is native-adapter coverage, not a Linux desktop test.

Final macOS workspace validation: **209 Rust tests passed, two timing benchmarks
ignored**, warnings-denied all-target/all-feature Clippy, `cargo ci-fmt`, **46 frontend
tests**, production frontend build, **11 Python tests**, and `git diff --check` passed.
The optional recording harness builds in release mode; its cap/channel-order and WAV
format tests are included in the Rust total. No GUI or conferencing test is claimed.

Fresh paced release AEC + real DFN graph runs measured 1,000 frames each after warm-up:

| Injected CPU load | p50 / p95 / p99 / max processing time | Missed 10 ms deadlines |
| --- | --- | --- |
| None | 2177 / 3212 / 3663 / 5698 µs | 0 |
| Two load threads | 507 / 659 / 772 / 1297 µs | 0 |

Both had zero native errors or resets, with ten startup fallback frames and 1,090 AEC
frames including warm-up. These software-graph runs do not establish hardware latency
or sustained scheduling reliability, and do not explain or fix the six misses observed
in the earlier no-load run. The new native `new`/`new[]` probe observed zero steady-state
processing allocations in 1,000 frames on each platform; construction/reset still
allocate and C allocation functions are unmeasured.

Software quality passing does not enable desktop AEC automatically.
Physical processed-voice listening, actual conferencing, independent desktop
incoming/outgoing virtual buses, Bluetooth/profile transitions, device recovery,
and Phase 6's two-hour concurrent hardware tests remain open.

The profile was selected against synthetic fixtures. Nonlinearities and room
responses outside these fixtures may expose additional tradeoffs. In particular,
the quieter independent-stereo unvoiced gate has limited headroom above its gain
floor on macOS. No acceptance target was lowered to hide that limitation. The
aligned fallback remains unprotected microphone audio when a valid reference is
unavailable; desktop behavior for that state must be completed before enablement.
