# Phase 6: Incoming speech processing

## Result

Phase 6's software path implements E15-01 through E15-07: native application PCM, opt-in incoming
speech processing, independent stereo histories, macOS/Linux loopback, recapture prevention,
two concurrent route classes, and microphone priority. A short macOS BlackHole-to-AirPods listening
and routing check passed on 2026-09-09. Broader hardware listening, physical full-duplex latency,
and long soaks are still acceptance gates; see [the hardware procedure](PHASE_6_HARDWARE_TEST.md).

## Sources and routing

| Source | Destination | Lifetime and support |
| --- | --- | --- |
| Rust application PCM | Explicit physical output | Bounded, continuous float32 mono/stereo stream; 8–192 kHz |
| BlackHole 2ch capture on macOS | Verified physical headphones | Existing driver; select BlackHole as the media/call application's output |
| OpenKrisp Incoming Audio on Linux | Verified physical headphones | Separate owned PipeWire sink/source pair, created on Start and removed on Stop |

The loopback path captures audio deliberately sent to the selected virtual endpoint. It does not
capture a physical speaker monitor, inspect arbitrary application graphs, or change system defaults.
Aggregate, multi-output, unknown and virtual render destinations fail closed. Native IDs and
ancestry are checked on startup and every existing headless recovery generation.

Linux uses `linux:pipewire-incoming`, `pipewire:openkrisp_incoming_sink`, and
`pipewire:openkrisp_incoming_source`, independently of the virtual microphone's existing pair.
Creation uses fixed `pw-loopback` arguments and the existing owned child/process-death cleanup.
Pre-existing valid endpoints are reused; incomplete pairs fail validation. No PCM crosses a
subprocess pipe. macOS uses the existing `macos:blackhole-2ch` identity and mirror ancestry checks.

BlackHole 2ch cannot carry incoming audio and the virtual microphone at the same time. Both
directions of that concurrent request are rejected before starting another owner. macOS can run
an incoming route with a physical microphone monitor using a separate physical output, or with
the call application's direct microphone. A separate macOS virtual path is future work. Linux's
two distinct pairs support the virtual microphone plus incoming route together. Two OpenKrisp
routes cannot concurrently own the same logical destination.

## Native PCM API

`audio_engine::application_pcm_stream(format, capacity_ms)` creates a move-only
`ApplicationPcmSource` and single `ApplicationPcmWriter`. Capacity is limited to 1–1000 ms;
the format is validated before allocation. Create configuration with
`RouteConfig::application_stream(...)`, which defaults to `AlignedBypass`, and submit the source
with `AudioEngineHandle::start_application_route(config, source)`.

Wait for `writer.is_ready()` before producing paced, complete interleaved frames. `write()` is
allocation-free and nonblocking. Inspect `WriteReport.written_frames` when the ring is full;
newest excess frames are rejected and stale queued input is bounded by the worker's policy.
The worker polls at its existing 2 ms idle interval. A producer can prepare data on its own thread;
it never receives a model or native output handle.

Stop/Quit closes the acceptance and playback gates, joins the DSP worker, and drops output on
the route owner. A disconnected producer, terminal output error or worker exit produces an
observable failure and releases resources. Restart an application route with a fresh source after
output loss; this source is not silently rebound during device recovery. This is a continuous live
PCM API, not a file decoder/player or a finite-stream EOF/drain API. Explicit Stop has the existing
immediate-silence semantics and can discard buffered tail audio.

`EngineCommand` is now move-only because its native application variant owns the single-consumer
ring. No Tauri command accepts this variant or PCM. The existing `start_route` IPC gains only an
optional `sourceKind: physical_input | system_loopback`; omitted `mode` defaults to speech for a
microphone and bypass for loopback. Unknown fields, PCM payloads, application-stream source kinds
and incoming-to-virtual-destination requests are rejected. No command or capability is added.

## Channels and worker behavior

The pinned asset remains a mono 48 kHz model with 480 samples per frame and 480 samples of
algorithmic delay. `denoise_core::IndependentChannels` composes one mono processor per channel.
Incoming mono stays mono (and may duplicate to stereo output); incoming stereo keeps left/right
independent. More than two source channels and stereo-to-mono rendering are rejected. The existing
microphone downmix policy is unchanged.

The worker keeps its rings, resamplers, frame assembler, dry delay and crossfade interleaved.
Preallocated planar buffers bridge only the reducer call. All offsets and progress counts now
distinguish frames from scalar samples. Model creation, warm-up, settings, inference, resets and
destruction stay on the constructing DSP thread. Histories are never shared between routes or
channels. Stereo costs two mono inferences; the measured deadline covers the whole stereo frame.

Incoming bypass skips inference and preserves independent delayed channels. After bypass or
priority suspension, the model resets once before inference resumes. Discontinuities during
suspension reset the DSP timeline and defer model reset until resumption. The final safety limiter
and independent-clock resampling still apply, so bypass is not a bit-exact/offline archival mode.

## Concurrent ownership and priority

The supervisor has two bounded slots: one physical microphone and one incoming source. Each has
its own native owner, worker, rings, model state, recovery state and metrics. Commands resolve by
route ID; snapshots retain one active/terminal observation per class. Duplicate IDs and conflicting
paths are rejected. Quit signals every owner before joining any owner.

At the existing 100 ms default metrics cadence, microphone startup/recovery, degraded state, new
missed deadlines or new queue xruns suspend incoming inference. Incoming resumes after two quiet
seconds or when the microphone stops. This does not modify the user's selected bypass state;
the existing `degraded` and skipped-inference metrics explain the effective fallback. It is a
cooperative CPU policy, not an OS scheduling guarantee; a frame already running and native model
construction cannot be preempted. The existing per-route overload guard remains active.

## UI

The incoming panel provides endpoint/output selection, setup instructions, speech/bypass mode,
Start/Stop, live bypass control, output meter, latency, p99 and degradation status. It defaults to
bypass and reconciles the incoming route independently of microphone snapshot order. Both panels
explain shared-endpoint/output conflicts. Routing snapshots add a serde-defaulted
`loopbackEndpoints` list; older snapshots still render safely.

## Verification

Final macOS workspace result: **160 Rust tests passed**, with one pre-existing opt-in timing
benchmark ignored; **26 frontend tests passed**. Formatting, warnings-denied Clippy, TypeScript
checking and the production frontend build passed. Linux's native audio/router suite and private
PipeWire transfer/cleanup suite also passed.

- Locked workspace tests, warnings-denied Clippy, formatting, frontend typecheck/build and tests.
- Deterministic arbitrary-chunk mono/stereo processing at 48→48 kHz and 44.1→32 kHz: each stereo
  channel equals its separately processed mono reference, wet/dry durations agree, expected
  sample-rate duration remains within the bounded resampler/frame tail, and gain is preserved.
- Allocation counter observes zero allocations in those tested incoming steady-state loops.
- Priority tests exercise suspension, deferred discontinuity resets, one reset on resumption and
  the quiet-period release policy. Lifecycle tests run, control, stop, restart and join both classes.
- The pinned production model processes a deterministic voiced-harmonic/noise fixture: stereo
  left equals an independent mono reference, silent right stays silent, and reset clears history.
  This verifies temporal/channel integrity, not subjective enhancement quality.
- Native ARM64 Debian 12 audio/router tests and warnings-denied audio/router/engine Clippy pass.
  The private PipeWire 0.3.65/WirePlumber 0.4.13 session transfers distinct stereo signals through
  both endpoint pairs concurrently without cross-capture. Two incoming creation/removal cycles
  leave the microphone running. Existing Stop and destructor-free termination cleanup tests pass.
- Tauri invoke tests verify incoming bypass defaults, metadata-only payloads and unsafe-request
  rejection. Frontend tests cover mode defaults, old/missing endpoint states, conflicting routes
  and independent incoming rendering.

Linux reproduction: `dbus-run-session -- bash scripts/test-linux-pipewire.sh` with the dependencies
documented in the Phase 5 report. The private session never opens physical audio devices.
Actual conferencing apps, Bluetooth profile changes and two-hour concurrent hardware runs remain
unexercised. The existing opt-in production timing benchmark remains excluded from routine tests.

### macOS listening run — 2026-09-09

The `macos_incoming_listening` example exercised the production `AudioEngine` SystemLoopback
route on an Apple M3 Max running macOS 26.6.2 (25G83), with BlackHole 2ch 0.7.1 as capture and
AirPods Pro as the explicitly selected physical output. Both streams negotiated 48 kHz stereo
float32 with 512-frame buffers. The model was `deepfilternet3-low-latency-v0.5.6`, with a 40 ms
route target. No physical microphone was opened and system input/output defaults stayed unchanged.

The roughly 41-second run played a 16.5139-second synthetic noisy speech fixture in aligned
bypass, repeated the same fixture with denoising, then returned to bypass for a left-only tone
followed by a right-only tone. Signal appeared in every phase, inference was skipped in initial
bypass, and enabling denoising caused one model reset. Both mode transitions succeeded. Stop
reached `Stopped`, the fixture stream was dropped, and engine shutdown joined its owners.

- Zero missed deadlines, input overruns, output underruns, discontinuities, stream issues,
  callback violations, model errors or recovery attempts were recorded.
- The highest sampled rolling inference p95/p99 values were 3.709/4.044 ms. These are sampled
  rolling statistics, not whole-run percentiles.
- Sampled engine latency estimates ranged from 37.1 to 57.2 ms. This does not measure acoustic
  end-to-end latency or the AirPods Bluetooth transport delay.
- The listener confirmed both clips were audible, the second had less noise with clear speech,
  and the final tones played in the correct left/right order.

This short routing/listening check passed. It does not establish a hardware support tier,
long-run stability, physical microphone concurrency, device recovery, conferencing compatibility
or objective perceptual quality. Local raw observations and the machine-readable result are in
`target/phase6-listening/macos-live-2026-09-09.log` and the adjacent `.json` file.

## Verified references and next work

The implementation reuses the locked local CPAL, Rubato and DeepFilterNet interfaces. On
2026-09-08 the upstream [PipeWire loopback documentation](https://docs.pipewire.org/page_module_loopback.html)
confirmed separate virtual sink/source construction and channel ordering. The
[BlackHole integration documentation](https://github.com/ExistentialAudio/BlackHole)
documents directing application output into a loopback device.

Complete Phase 6 hardware acceptance before claiming a supported latency/CPU tier. The subsequent
[Phase 7 slice](PHASE_7_RENDER_REFERENCE_IMPLEMENTATION_REPORT.md) implements E16-01/E16-02
(render reference and timing plumbing). E16-03 AEC evaluation is next; acoustic echo cancellation
remains outside this phase and headphones remain the supported acoustic arrangement.
