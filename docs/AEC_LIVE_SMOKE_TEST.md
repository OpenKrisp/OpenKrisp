# Native AEC acoustic smoke test — 2026-09-11

The real native audio path worked on a MacBook Pro running macOS 26.6.2 (25G83),
ARM64/CoreAudio. A 60-second quiet-listening run and a separate 20-second simultaneous
speech run completed. The user confirmed hearing the speaker fixture and speaking
during the second run. This is a short hardware smoke test, not E16-05 acceptance.
It ran against `26fd257`, before the [explicit-channel AEC fix](AEC_QUALITY_INVESTIGATION.md);
that subsequent native change has not been retested on physical devices.

## Path and procedure

```text
Native stereo fixture → incoming AudioEngine route → MacBook Pro Speakers
                                                       ↓ room acoustics
MacBook Pro Microphone → native AEC3 → DFN/bypass → BlackHole 2ch → CPAL receiver
```

The desktop routes were stopped. The harness opens explicit devices, without changing
system defaults. It uses a native application PCM source, so the single BlackHole bus
can carry microphone output without feeding it back into incoming capture. This does
not lift the desktop restriction on sharing BlackHole between both routing directions.
The microphone headless owner and incoming application-route owner share native audio
taps; this harness does not exercise the desktop supervisor's microphone priority policy.

- Speakers: `coreaudio:BuiltInSpeakerDevice`, stereo 48 kHz float32.
- Microphone: `coreaudio:BuiltInMicrophoneDevice`, mono 48 kHz float32, 512-frame buffer.
- Microphone output: `coreaudio:BlackHole2ch_UID`, stereo 48 kHz float32, 512-frame buffer.
- Receiver: independent CPAL input on BlackHole, stereo 48 kHz. The mono microphone is
  duplicated to both channels; equal receiver levels are not a stereo-order acceptance test.
- Model: `deepfilternet3-low-latency-v0.5.6`, with the pinned native WebRTC AEC3 prototype.

The existing `target/phase6-listening/processed.f32` speech fixture was loaded in Rust
and attenuated by 0.5 before playback. Its hash and duration are in
[the evidence manifest](evidence/aec-live-2026-09-11.json). Speaker volume and room geometry
were not instrumented. No microphone recording was made; only metadata and aggregate levels
were saved. No PCM crossed Tauri IPC.

The feeder writes 480 stereo frames every 10 ms to a 100 ms native application queue.
The receiver callback copies complete frames into a 200 ms bounded ring; the owner drains
it and computes levels. Loading, pacing, calculations and logging occur outside callbacks.
Production stream callbacks retain their existing bounded, nonblocking behavior.

The first run repeats the fixture in three 20-second phases: both output paths bypassed,
AEC with microphone DFN bypassed, then AEC with incoming and microphone DFN enabled.
The microphone stops/restarts between phases while incoming stays running. Microphone
DFN remains warm during its bypass, so bypass does not imply zero inference cost.
The `--talk` run uses only the final AEC + DFN phase while the user speaks.

## Observations

Receiver RMS is computed over approximately seconds 10–20 of each phase, after startup
and initial convergence. Values are normalized float32; left/right values were identical.

| Phase | Receiver frames in level window | Receiver RMS per channel | Active AEC frames at result | Initial fallback frames |
| --- | ---: | ---: | ---: | ---: |
| Quiet, AEC off / DFN bypassed | 479,744 | 0.00231402 | 0 | 0 |
| Quiet, AEC on / DFN bypassed | 480,256 | 0.000393859 | 1,989 | 42 |
| Quiet, AEC + incoming/microphone DFN | 479,744 | 0.000194970 | 1,991 | 42 |
| User speaking, AEC + incoming/microphone DFN | 480,256 | 0.00330700 | 1,991 | 41 |

The quiet AEC-only receiver level was **15.38 dB below baseline**; AEC + DFN was
**21.49 dB below baseline**. These are aggregate receiver-level comparisons across separate
playbacks, including room noise, filtering and different pipeline delays. They are not
isolated echo ERLE, a near-end preservation score, or measured end-to-end latency.
The speaking run demonstrates signal transfer during double talk; its RMS does not establish
voice intelligibility or lack of attenuation. The processed microphone was not listened to.

AEC reported `Running` at every one-second active-phase observation, with no additional
fallback after initial alignment, no native errors, no AEC resets and no AEC-only deadline
misses. The reported maximum AEC processing times were 2,856 / 1,423 / 1,387 µs for the
three AEC phases. AEC entered `Stopped` after each microphone Stop; restarting the microphone
reacquired the still-running incoming reference successfully.

**One combined microphone processing deadline was missed** in the AEC-only output phase
(DFN still runs while bypassed). The other phases reported zero. Route snapshots reported
zero input overruns, output underruns, stream issues, callback violations, model errors,
capture discontinuities and recovery attempts. Both feeder and receiver dropped-frame
counters were zero at every phase result. Incoming recorded one expected model reset when
switching from bypass to denoising, with zero incoming deadline misses.

The largest sampled microphone rolling p99 was 6,311 µs in the first run and 5,802 µs in
the speaking run; these are maxima of rolling diagnostics, not whole-run timing percentiles.
The retained runs sampled microphone route health once per second, rather than at final
shutdown; the last fraction of each phase is not independently covered by those snapshots.
The example now also prints a final microphone snapshot and receiver callback/issue counters.
Both processes exited successfully and reported all test routes stopped. Exit success means
sequence completion; diagnostic review and quality acceptance are separate.

## Reproduction and remaining work

Build with the native AEC tools described in [the prototype report](AEC_PROTOTYPE_IMPLEMENTATION_REPORT.md).
Stop desktop routes and other audio first; use comfortable low speaker volume. The fixture
must be 1–60 seconds of finite stereo float32 LE at 48 kHz, within ±0.5 before attenuation.

```sh
cargo build --locked --release -p audio-engine --features aec-prototype --example aec_live
target/release/examples/aec_live INPUT_STEREO_48K.f32 \
  coreaudio:BuiltInSpeakerDevice coreaudio:BuiltInMicrophoneDevice coreaudio:BlackHole2ch_UID
# Repeat just the final 20-second phase while speaking:
target/release/examples/aec_live INPUT_STEREO_48K.f32 \
  coreaudio:BuiltInSpeakerDevice coreaudio:BuiltInMicrophoneDevice coreaudio:BlackHole2ch_UID --talk
```

Run with macOS audio access; Ctrl-C stops the finite test. Raw metadata logs remain under
`target/aec-evaluation/live-hardware-2026-09-11.log` and `live-talk-2026-09-11.log` locally.
The checked-in manifest retains summaries and hashes without microphone PCM.

Desktop AEC remains disabled. This did not test a conferencing application, simultaneous
desktop virtual-microphone and loopback routing, acoustic latency, processed-voice listening,
speaker-volume/path changes, device recovery, Bluetooth profiles, or two-hour stability.
The broadband double-talk fixture failed at this revision; the later
[software quality fixes](AEC_QUALITY_FIXES.md) do not update this historical hardware result.
E16-05/E16-06 and
Phase 6's previously open hardware/conferencing gates remain open; headphones remain the fallback.

## Optional processed-microphone recording in the current harness

After explicit agreement to recording and a retention/deletion choice, replace `--talk`
with `--talk-record NEW.wav` for one 20-second AEC + DFN phase. The file must not already
exist. This saves only the processed virtual-microphone receiver output, never raw capture.
The owner preallocates at most 3.84 MB of stereo 48 kHz i16 samples, drains the existing
bounded receiver ring, and writes the private (0600 on Unix) WAV after all routes stop.
No callback allocates or writes files. PCM stays local and outside Tauri IPC.
The default modes still record no microphone audio. An interrupted run can save a partial
clip; an early error can leave an empty new file. Review locally after shutdown and delete
according to the agreed retention choice. A single clip is only an initial listening check.
