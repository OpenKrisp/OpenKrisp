# E16-05 quality investigation and explicit AEC channels

**Historical investigation:** the failures, native settings and validation counts below
describe the earlier revisions. The subsequent [quality fixes](AEC_QUALITY_FIXES.md) resolve
the three synthetic failures and enable them in routine CI. Physical acceptance stays open.
Desktop AEC was later [enabled by default at the user's request](AEC_DESKTOP_DEFAULT_IMPLEMENTATION.md).

This follow-up to `26fd257` fixes an internal AEC3 stereo-reference bug and adds
reproducible quality diagnostics. **E16-05 remains open.** The original broadband
acceptance failure persists, and new synthetic fixtures expose weaker local-speech
attenuation and slow recovery after an echo-path change. Desktop AEC stays disabled.
No audio devices were opened and no microphone PCM was recorded during this investigation.

Follow-up on 2026-09-14/15: [speakerphone acceptance criteria](AEC_SPEAKERPHONE_ACCEPTANCE.md)
now define the desktop enablement decision and a dedicated command that includes
the failing gates. The added delay/gain investigation below changes tests and
diagnostics only; the native adapter's settings remain those described here.

## Stereo fix

Enabling APM's `pipeline.multi_channel_render` preserves its input planes, but AEC3
also has an internal content detector. Its default hysteresis requires more than
200 consecutive 10 ms stereo frames before activating multichannel processing.
Silence resets that streak. During the initial mono mode, opposite-phase channels
can cancel in the downmix. Speech with frequent pauses can remain in that mode
indefinitely; the earlier continuous-noise test only measured after the transition.

The native bridge now supplies an `EchoControlFactory` using upstream mono/stereo
defaults with `multi_channel.detect_stereo_content = false`. AEC3 therefore uses
the declared one or two render channels from construction. This removes the
content-triggered channel transition and retains stereo even through silence or
duplicated-mono content. Suppression, gain control and noise-suppression settings
are otherwise unchanged; none of the experimental tuning described below is adopted.

The factory, APM and DFN remain confined to the microphone worker. Capture is still
mono, render planes stay ordered, and frames remain 480 samples at 48 kHz. Native
generation ownership, bounded taps/history, timestamps, reset/fallback, Rust PCM
transport and callback constraints are unchanged. No tuning or PCM interface was
added to Tauri. The impulse-peak regression still reports 430 samples.

Source evidence is in the pinned archive listed in `third_party/aec/README.md`:
`multi_channel_content_detector.cc` (`UpdateDetection` and constructor),
`echo_canceller3.cc` (persistent-channel selection and render handling), and
`audio_processing_impl.cc` (`InitializeEchoController`). The bridge uses the same
`CreateDefaultMultichannelConfig()` preset selected by the previous APM factory.

| Deterministic native fixture | Previous adapter | Explicit channels |
| --- | ---: | ---: |
| Intermittent opposite-phase speech-like render, echo-only seconds 4–6 | 0.852 dB ERLE | 26.134 dB macOS / 26.135 dB Linux |
| Same fixture, seconds 8–10 | Not measured before fix | 25.410 dB macOS / 25.400 dB Linux |
| Continuous independent stereo noise, 40/100/160 ms delays | 18.06 dB | 17.15 dB; all above unchanged >10 dB floor |
| Pulsed duplicated-mono / opposite-phase noise, macOS | New regression | 24.029 / 23.833 dB |

The new pulsed test uses 400 ms sound / 200 ms silence. The speech-like generator
alternates harmonic, noise-like and silent sections at independent pitches/cadences.
The intermittent-stereo and stronger-local-speech cases passed natively on macOS
ARM64 and Linux x86_64; the additional pulsed regression was added after the Linux
run and validated on macOS. These fixtures do not establish speech intelligibility.

## Where local-signal attenuation occurs

`scripts/probe-aec-suppression.py` compiles a separate diagnostic against the pinned
native archive. It observes AEC3's 16 kHz linear-filter output and APM's final 48 kHz
output, each compared with a **same-stage, same-configuration near-only baseline**.
It never compares the two sample rates directly or uses impulse alignment as a
substitute for the filterbank's frequency-dependent phase. Far-end echo is evaluated
over seconds 4–6 and local preservation over seconds 7–10 of the original broadband
fixture. Export requires flags in both APM and AEC3; neither is enabled in production.

| Diagnostic profile, macOS | Echo-only ERLE | Final local projection gain / correlation | Linear local gain / correlation |
| --- | ---: | ---: | ---: |
| Previous automatic channels | 20.701 dB | 0.368 / 0.734 | 1.000 / 0.998 |
| Explicit channels, adopted | 17.366 dB | 0.380 / 0.742 | 1.000 / 0.998 |
| Stationarity trial, rejected | 7.646 dB | 1.000 / 0.957 | 1.000 / 0.998 |
| High-frequency masking/ERLE trial, not adopted | 17.144 dB | 0.762 / 0.910 | 1.000 / 0.998 |

Linux reproduced the diagnosis. The linear stage preserves the local signal; most
loss occurs downstream in the residual-suppression path. Changing the residual
suppression behavior changes final preservation while leaving the linear result
unchanged. Relevant pinned code: `EchoRemover::ProcessCapture`,
`SuppressionGain::GainToNoAudibleEcho`, `UpperBandsGain` and `DominantNearendDetector::Update`.

The stationarity trial drops below the unchanged >10 dB echo floor. The masking trial
copies low-frequency near-end masking thresholds into the high-frequency settings
and raises `erle.max_h` to `erle.max_l`. It passes the original broadband gate, but the
wider Rust matrix still fails: weaker independent-stereo local speech has voiced gain
0.611 and unvoiced gain 0.373, while a delay/gain step yields only 6.054 dB echo reduction
in the recovery window. These experiments are retained solely in the standalone probe;
changing thresholds to pass one fixture would not establish voice quality.

## Acceptance gates that remain negative

Targets were not lowered: local gain must be in `[0.7, 1.5)`, correlation >0.8, and
settled echo-only ERLE >10 dB. For new speech-like fixtures, voiced and unvoiced
sections are scored separately so stronger vowels cannot hide missing noise-like
segments. Local amplitude scales 0.5 / 1 / 2 are fixture multipliers, not measured
signal-to-echo ratios. The original broadband target remains unchanged.

- **Stationary broadband:** gain 0.380, correlation 0.742 after the channel fix.
- **Weaker speech-like double talk:** at scale 0.5 with independent stereo render,
  voiced gain/correlation 0.578/0.811; unvoiced 0.265/0.612. At scale 1, the same
  unvoiced gate still fails at 0.542/0.792. All three render layouts pass both
  component gates at scale 2 (unvoiced gains 0.837–0.865, correlations 0.931–0.945).
- **Echo-path step:** at six seconds, delay changes from 100 to 160 ms and the left
  echo gain from 0.65 to 0.35. The fixed seconds 8–10 recovery window scores 6.997 dB
  for independent stereo (7.010 dB on Linux), below the >10 dB engineering target.
  The opposite-phase case passes at about 20.97 dB. This is an acoustic-path fixture;
  route/device recovery and timestamp discontinuity behavior were not changed.

All three acceptance tests are explicitly ignored in routine CI and were run to
failure on both native targets. They remain visible regression targets, not waived
acceptance. The passing stronger-local test is deliberately labeled for its limited
scope. The gate suite is synthetic engineering evidence, not a listening score or
proof that all human speech at a given volume will behave the same way.

## Delay-change diagnosis (2026-09-14/15)

The diagnostic now accepts `--path-changes` and separates no change, gain-only,
delay-only and combined changes across mono/stereo and broadband/speech-like input:

```sh
python3 scripts/probe-aec-suppression.py debug --path-changes
```

It uses the existing default explicit-channel configuration. It exports the linear
stage only in this diagnostic, compares that stage with a paired linear baseline,
and samples APM's `delay_ms` after each capture frame. APM's statistics handoff has
capacity one; it must be drained from startup so the first observation in a later
measurement window does not contain an old startup value. No statistics or PCM
interface was added to the production adapter or Tauri.

In the independent-stereo speech-like fixture, the final seconds 8–10 show:

| Change at six seconds | Final echo-only ERLE, Rust regression / native probe | Probe linear ERLE | Probe buffer-delay range |
| --- | ---: | ---: | ---: |
| None | 24.452 / 24.452 dB | 28.861 dB | 96–96 ms |
| Left echo gain 0.65 → 0.35 only | 21.296 / 21.296 dB | 21.828 dB | 96–96 ms |
| Echo delay 100 → 160 ms only | **9.735 / 9.734 dB** | 4.649 dB | 96–156 ms |
| Both changes | **6.997 / 7.005 dB** | **−0.271 dB** | **96–96 ms** |

These are macOS measurements. The standalone C++ generator follows the Rust
fixture's formula but floating-point results differ slightly; the diagnostic is
not the acceptance oracle. The Rust assertions retain their unchanged thresholds
and scoring windows. All three gain-only render layouts now pass a normal CI
regression. The ignored path-change gate includes delay-only independent stereo
and a passing mono combined-change control as well as its original two cases.

The combined failure includes stale delay alignment and ineffective linear
cancellation, separately from the double-talk residual-suppression failure above.
The reported 96/156 ms values are AEC3's **software render-buffer estimates**, not
the synthetic path's exact 100/160 ms delay or measured capture-to-output latency.
Delay-only recovery is late enough that the fixed scoring window still fails.

Pinned-source inspection shows that `AlignmentMixer` selects one render channel
for delay estimation, and `MatchedFilter::Update` requires its residual error to
be below a configured fraction of capture energy before accepting a candidate.
Interference from the other speaker channel is a hypothesis for further
investigation; these observations do not identify every internal rejection or
justify relaxing the delay-confidence threshold. No AEC parameter changed in
this follow-up. A prospective fix must also reject false alignment during local
speech and preserve the earlier opposite-phase regression.

The dedicated `scripts/check-aec-quality.py` runs the actual compiled native test
executables, including ignored tests, checks their required inventory and rejects
skipped/empty runs. It records source hashes and logs even when a target fails,
and continues through the other targets. On macOS it ran **12 native tests: nine
passed, three failed, none ignored**. The failures are the retained acceptance
gates, not a successful quality result. Its own regression tests cover absent
gates, filtered/ignored/empty execution and failure-log retention.

Latest macOS workspace checks: **201 Rust tests passed, five ignored**, 46 frontend
tests passed, Clippy with warnings denied, formatting, production frontend build,
10 Python tests and `git diff --check` passed. Linux x86_64 rebuilt the pinned
adapter, passed all nine routine native tests (three ignored), ran both native
diagnostics, and then reproduced all three failures with the dedicated runner:
nine passed, three failed, none ignored. Its delay-only/combined stereo gates
measured 9.651/7.010 dB, with the same late/stale buffer-delay observation. The
disposable container exited 1 because quality acceptance failed, not because
compilation or the normal native regressions failed.

The [new evidence manifest](evidence/aec-speakerphone-gates-2026-09-15.json) retains
results and source/log hashes separately from the 2026-09-11 measurements. No
hardware, production-graph timing, microphone recording or conferencing run was
performed for this follow-up. Linux's existing allocation/timing probe also ran
as part of its native script; it is not a new worker or hardware timing result.

## Timing and native allocation observations

The existing release graph benchmark now accepts `OPENKRISP_AEC_PACED=1`, which
schedules one frame every 10 ms without a catch-up burst after a stall. A bounded
optional pair of CPU load threads remains available. It uses real AEC + DFN with
synthetic timestamps and a device-free sink; it does not reproduce CoreAudio or
measure capture-to-playback latency. The previous live deadline miss lacks enough
per-frame CPU/scheduler evidence to identify its cause or claim it is fixed.

The final paced measurements and host-work context are recorded in
[the evidence manifest](evidence/aec-quality-2026-09-11.json). The earlier paced pair
overlapped Linux container setup/build and is retained separately. No zero-hardware-
deadline claim follows from these short diagnostics. The final pair ran after the
container and workspace checks finished:

| Paced AEC + DFN, 1,000 measured frames | p50 / p95 / p99 / max (µs) | >10 ms processing misses |
| --- | --- | ---: |
| No injected CPU load | 2523 / 5337 / 7021 / 25064 | **6** |
| Two bounded CPU load threads | 584 / 1276 / 1472 / 2096 | 0 |

Both runs had 1,090 active AEC frames, 10 initial alignment-fallback frames and no
native/model errors or resets. AEC-only deadline counters stayed zero. The benchmark
completed as a diagnostic; the six misses are **not** a performance pass. Faster
results under load do not identify the cause: per-frame thread CPU time, scheduler
preemption and power-state effects were not measured. Timing remains open alongside
quality; no additional physical-device test was run to claim a hardware fix.

With explicit channels the standalone native probe counted 5,217 construction
`new`/`new[]` calls requesting about 2.25 MB cumulatively, and **zero** such calls in
1,000 processing frames after 100 warm-up frames, on macOS and Linux. The earlier
665 calls in that window were measured with automatic content detection, whose
two-second channel reconfiguration fell inside the nominal steady measurement.
Direct C allocation functions remain unmeasured; these are allocation-traffic
observations, not live/peak heap bounds or an allocation-free native-worker claim.

## Reproduction and validation

After the native tools are installed as described in the prototype report:

```sh
cargo test --locked -p aec-webrtc -- --nocapture
python3 scripts/probe-aec-suppression.py
# Expected failure: all three unresolved gates, without stopping after the first target.
cargo test --locked -p aec-webrtc --no-fail-fast -- --ignored --nocapture
# No audio devices are opened by this paced diagnostic.
OPENKRISP_AEC_PACED=1 cargo test --locked --release -p audio-engine \
  --features aec-prototype production_aec_dfn_graph_timing -- --ignored --nocapture
```

`cargo ci-test --locked`: **200 passed, 5 ignored**. Ignored: three quality gates,
the pre-existing DFN wall-clock recovery check, and the release AEC + DFN benchmark
(run explicitly). Workspace Clippy with warnings denied, formatting, 46 frontend
tests, production frontend build, six Python tests and `git diff --check` passed.
The Linux script also runs the new suppression diagnostic; native regression tests
passed and the separately invoked acceptance command failed on the three known gates.
Raw logs are local under `target/aec-evaluation/quality-*` and the manifest retains
summaries, source hashes and validation scope.

The previous live smoke test used `26fd257` and **predates this native change**.
Processed-voice listening, a repeated hardware run, volume/path changes in a room,
actual conferencing, Bluetooth, device recovery and Phase 6's concurrent two-hour
tests remain open. Headphones remain the fallback; desktop AEC is still disabled.
