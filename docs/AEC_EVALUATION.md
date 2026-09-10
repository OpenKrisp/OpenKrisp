# AEC evaluation and prototype design — E16-03

Follow-up: [E16-04's implementation report](AEC_PROTOTYPE_IMPLEMENTATION_REPORT.md) records
the subsequent native build, integration, measured results, design adjustments and unresolved
quality acceptance. This evaluation remains the source-only evidence at the selection point.

## Decision and scope

**Select WebRTC AEC3 through `webrtc-audio-processing` 2.1.0 as the E16-04 prototype
candidate.** Use its full echo canceller in a microphone-worker-owned adapter, ahead of
DeepFilterNet. This is a source-backed engineering selection, conditional on the build,
allocation, timing and quality gates below. It is not an echo-quality ranking or approval
for open-speaker use.

Evaluation date: **2026-09-10**. Starting point: clean `main` / `origin/main` at `e794806`
(PR #11). This change completes E16-03 only. It adds documentation and source provenance;
no AEC dependency, native capability, public API, PCM transport or runtime behavior changes.
E16-04 through E16-06 and the previous hardware acceptance tasks stay open. The numbered
backlog decreases from 14 to **13**.

## Evidence and candidate comparison

The downloaded published wrapper and sys archives match the official crates.io sparse-index
SHA-256 values and were not yanked when inspected. The wrapper's tagged Git tree identifies
native submodule `d0569cfa50c1858ee279d77b3fc8870be6902441`. The inspected native files are
those actually shipped in the sys archive; do not replace them with an arbitrary system
library merely reporting version 2.1. Archive checksums, file hashes, source URLs and exact
revisions are in [the evidence manifest](evidence/aec-evaluation-2026-09-10.json).

To repeat the source audit, download the manifest's exact archive URLs, compare SHA-256
against the recorded values (and the matching 2.1.0 entries in the official registry index),
extract without executing build scripts, then check the listed source-file hashes relative
to each source root. The native source root is `webrtc-audio-processing/` inside the sys
archive. Abseil source/patch hashes are recorded from its wrap; those archives were not
downloaded or independently verified here. Local source caches are ignored under
`target/aec-evaluation/`, and no third-party source is added to the repository.

| Candidate inspected | Relevant fit | Costs and unresolved evidence | Decision |
| --- | --- | --- | --- |
| [WebRTC Rust bindings 2.1.0][wap-release], commit `c14d7af1760baff83e8210fee336a0cae0faaa7d`, with bundled native APM | Planar f32, 10 ms frames, full AEC3 with internal delay estimation, configurable multichannel render; upstream workflow covers macOS/Linux | C++ build and notices; per-call Rust pointer-vector allocation; internal locks/reinitialization; local build and CPU/quality unmeasured | First prototype candidate |
| [Rust AEC3 port][rust-port], commit `8b6d0bad184eaf5b594adf808fb702b698b411ad` (manifest 0.3.2) | Rust implementation, low-level AEC3 and separate capture/render channels; avoids C++ toolchain | Low-level render insertion clones nested audio storage; graph API introduces another scheduler/packet ownership model; no parity or performance result established here | Reserve candidate if native build/maintenance cost proves unacceptable |
| [SpeexDSP 1.2.1][speex-api], commit `1b28a0f61bc31162979e1f26f3981fc3637095c8` | Small explicit C echo API, reset, configurable filter length, multichannel microphones/speakers | Public echo API takes i16; caller must align playback/capture and correct independent clocks; no comparative CPU/quality measurement | Useful independent baseline, not first integration |

WebRTC is chosen for the verified interface fit and a reusable upstream AEC3 implementation,
not an assumption that all WebRTC-derived implementations behave identically. The Rust port's
current source was inspected instead of relying on older documentation for its removed VoIP
wrapper. Its low-level render queue is capacity-limited, but insertion clones a nested frame;
Rust alone does not establish allocation-free processing. Its graph layer is unnecessary for
OpenKrisp's existing two-route lifecycle. [Sources: Rust render writer][rust-render],
[graph runtime][rust-graph].

Speex's `speex_echo_state_init_mc` accepts microphone and speaker counts; the header suggests
10–20 ms frames and 100–500 ms filter lengths. These are library guidance, not OpenKrisp
latency promises. Its paired `speex_echo_capture`/`speex_echo_playback` convenience API adds a
two-frame assumption; an evaluation baseline should use `speex_echo_cancellation` with
explicit alignment. Its manual warns about unsynchronized soundcard clocks, so external
clock compensation would still be required. [Sources: header][speex-api], [manual][speex-manual].

Platform-managed voice processing would change the device/route ownership boundary and require
different macOS and Linux integrations. It is not part of this common native adapter selection.
No proprietary service or cloud processing is needed for the selected prototype.

## API and build findings

The inspected [Rust processing API][wap-api] and [C++ bridge][wap-bridge] establish:

- `Processor::new(48_000)` uses 480 samples **per channel** per call. The native float API
  accepts planar values in `[-1, 1]`. Prevalidate sample counts and finite values; malformed
  dimensions otherwise reach assertions in the wrapper.
- Call `analyze_render_frame` on borrowed render planes, then `process_capture_frame` on
  the microphone plane. Analysis does not modify the reference. Do not process the reference
  a second time or send the analysis result back to the speaker.
- Configure `EchoCanceller::Full { stream_delay_ms: None }` initially. Explicitly enable
  `pipeline.multi_channel_render` for stereo: its Rust default is false. Capture remains
  mono using OpenKrisp's existing microphone channel mapping. Keep WebRTC noise suppression,
  gain control and capture amplification disabled so DeepFilterNet remains the denoiser.
  Set high-pass filtering explicitly and account for it in quality tests: the configuration
  comments say Full enforces it, but the actual conversion sets
  `enforce_high_pass_filtering: false`. Do not rely on that stale comment. Avoid experimental
  configuration features initially. [Configuration types][wap-config], [conversion][wap-conversion].
- The wrapper advertises `Send + Sync`, but OpenKrisp will construct, process, reset and drop
  AEC on the microphone worker, alongside that worker's separately owned DFN state. Do not
  share it with the incoming worker or call it from callbacks.
- Both frame helpers collect channel pointers into a new `Vec` per call. Passing stack
  sample arrays does **not** eliminate this allocation. E16-04 must provide a narrowly scoped
  fixed-channel borrowed-buffer bridge/patch, or explicitly document and measure a bounded
  worker-only allocation exception. No callback exception is allowed. Never pass owned
  temporary channel collections whose storage could be dropped while constructing pointers.
- `set_config` and `reinitialize` acquire internal locks; format/channel changes can
  reinitialize native state. Keep format and configuration fixed during a generation and
  serialize all calls on the owner worker. Single-thread ownership avoids application-level
  contention; it does not prove the native implementation lock-free or allocation-free.
- The bridge's reset entry discards the native `Initialize()` return code. Prefer creating
  a fresh configured processor on discontinuity until reset success and equivalence to a
  fresh instance are verified. Reset/reconstruction is worker work and can miss a deadline.

The [build script][wap-build] uses C++17, bindgen/libclang, Meson, Ninja, native archive tools
and `rust-objcopy` for bundled symbol rewriting. It builds native code under Cargo's output
directory. The native Meson project supports Darwin/Linux and ARM64/x86 targets; the wrapper
[CI configuration][wap-ci] includes macOS and Linux. This is source evidence of intended
support, not a successful OpenKrisp candidate build.

Bundled mode does not mean a self-contained, reproducible build. The inspected native Meson
file accepts system Abseil or a wrap fallback; the fallback selects **20240722.0** plus a
Meson wrap patch, each with a hash. Cargo.lock alone does not pin that system library or
control Meson's downloads. E16-04 must pin these artifacts, prevent accidental host-library
substitution and verify clean builds/packaged linkage on both targets. Do not rely on an
unversioned distribution `libwebrtc-audio-processing` package. [Native build][native-build],
[Abseil wrap][abseil-wrap].

Local tool preflight found Clang and the toolchain's `rust-objcopy`, but no `pkg-config`,
Meson or Ninja on PATH. No tools were installed and no candidate was compiled or benchmarked
in this evaluation. Preparing the reproducible native build is the first E16-04 gate.

## License findings and redistribution boundary

| Inspected component | Source finding | Prototype/release action |
| --- | --- | --- |
| Rust wrapper/sys and WebRTC native code | BSD three-clause notices; WebRTC also carries a separate patent grant | Retain applicable copyrights, conditions and disclaimers in source/binary notices, observe the endorsement restriction, and retain/review the patent text. [Wrapper license][wap-license], [native license][native-license], [patents][native-patents] |
| Native third-party code | Additional notices for Ooura, the Mark Olesen FFT, PFFFT/FFTPACK, RNNoise and the public-domain sqrt routine | Inventory the selected native build, including compiled but disabled processing modules; preserve applicable texts. The sys archive hashes these files in the evidence manifest. Do not label the entire bundle solely BSD from Cargo metadata. |
| Abseil fallback | Apache-2.0 release license; separate Meson patch archive | Pin both source/patch and review their notices in the actual packaged graph. [Abseil license][abseil-license], [wrap][abseil-wrap] |
| Rust AEC3 port | Cargo says `MIT OR BSD-3-Clause`; LICENSE assigns MIT to new contributions and WebRTC terms to derived portions, with a separate PATENT file | Do not interpret the manifest as permission to discard WebRTC notices or as proof that every modification is covered by the patent grant. [License][rust-license], [patent text][rust-patent] |
| SpeexDSP | BSD three-clause project notice | Retain source/binary notices; review any chosen FFT backend and wrapper dependencies before bundling. [COPYING][speex-license] |

The reviewed terms support selecting a local open-source prototype; no blocking copyleft term
was identified in these inspected candidate files. This is a scoped source/notice review,
not a completed redistribution review of a future binary. The repository's `deny.toml`
already allows BSD-3-Clause, MIT and Apache-2.0, but cargo-deny does not enumerate native
Meson subprojects or establish patent clearance. **E17-07 stays open.** Existing BlackHole
distribution obligations are separate and unchanged.

## E16-04 integration design

This section specifies the next implementation; none of this AEC path exists yet.

### Placement and reference selection

```text
physical microphone -> timestamped capture transport -> mono mapping -> 48 kHz
                    -> AEC3 -> DFN frame processing / aligned denoise bypass
                    -> existing output resampling, limiter and virtual microphone

incoming worker -> output ring -> CPAL submitted PCM -> bounded render tap
                -> clock compensation / planar 48 kHz reference -> AEC3 analysis
```

Use the explicitly associated **incoming route's physical output** as the acoustic reference.
It includes incoming speech processing or bypass, channel mapping, resampling, final limiting,
device quantization and actual underrun silence. It precedes OS mixing, volume, device effects
and acoustics. Audio another application sends directly to speakers is missing from this
reference; the product must not claim whole-system echo cancellation in that topology.

A microphone's virtual output is not a speaker reference. Do not infer association from the
default device, use the microphone monitor as an automatic reference, or mix routes. Linux's
separate PipeWire pairs provide the intended conferencing topology. macOS BlackHole 2ch is
still exclusive to one virtual direction; native application-owned incoming PCM can support
an isolated prototype, but does not establish two-way conferencing compatibility or justify
bypassing existing conflict checks.

The critical capture prerequisite is visible in [transport.rs](../crates/audio-engine/src/transport.rs)
and [worker.rs](../crates/audio-engine/src/worker.rs): the primary input ring contains f32
samples only, and the worker may trim stale capture before processing. The separate raw tap
can observe samples that this ring drops. **Do not align the tap's timestamps to an independently
drained sample ring by assuming they contain the same samples.** Add a bounded timestamp-bearing
primary capture transport for the AEC path, with PCM and metadata accepted/dropped atomically,
and propagate frame position/epoch through mapping, resampling and assembly. AEC output must
feed the real microphone pipeline exactly once. Leave non-AEC routes on their established path.

### Ownership, clocks and delay

The microphone worker owns its AEC, capture clock tracker, reference resampler, frame scratch
and optional sole render reader. An owner/control coordinator takes readers from
`NativeAudioTaps` outside the processing loop and hands over replacements through a bounded
command path. Acquisition needs an explicit unavailable/busy result if another native consumer
already owns the sole reader. Pending replacement handoff must not block Stop. Never call the
registry's mutex-protected `take` from a callback or processing loop. An association grants
read access, not ownership of the incoming streams or DFN state.

Preserve native `stream_id`, `epoch`, frame positions and the current timestamp units:
unsigned callback-arrival nanoseconds and signed capture/predicted-playback nanoseconds on
the process-local monotonic clock. Device timestamps have separate opaque origins. Canonical
AEC frames are 480 samples/channel at 48,000 Hz, with input sample positions and resampler
delay tracked explicitly. Convert nanoseconds to milliseconds only at an API boundary that
actually requires it.

Resample the reference onto the microphone's sampling timeline using the existing independent
clock fits, retaining stereo channel order. This is separate from each route's output-fill
drift controller; it must not steer or reset the incoming route. For the first prototype use
the tested ±750 ppm stress cases, a proposed ±1,000 ppm compensation bound, and bypass AEC
when timestamps/rate estimates are unavailable or outside that bound. These are prototype
limits, not supported-hardware specifications. No capture wait is introduced while the
100 ms clock-fit observation span becomes available.

Feed chronological, rate-corrected render frames; let AEC3 estimate residual echo delay.
Do not first shift the reference by a guessed acoustic delay and then apply the same delay
again inside AEC. `render_frame_for_capture` maps contemporaneous estimated times, not the
earlier speaker sample that caused a particular microphone echo.

Do **not** pass `DuplexDelayEstimate::io_delay_ns` directly as WebRTC's stream delay. The former
is capture age plus render lead at CPAL callback arrival and excludes worker queues. WebRTC's
documented delay refers to analysis/processing call times:

```text
stream delay = (predicted playback time - render analysis call time)
             + (capture processing call time - estimated capture time)
```

Those times must refer to the corresponding audio frames, including reference buffering and
resampling. Analysis of an already submitted render block can occur after predicted playback,
so one term can be negative. An explicit-delay mode would need separate range/uncertainty
validation. Keep `stream_delay_ms: None` initially; do not force an estimate through a
nonnegative API by clamping it. Neither these values nor AEC's estimated acoustic delay are
**measured end-to-end latency**. [Native delay contract][native-api].

### Bounds and discontinuities

Retain the existing tap caps: 64 slots, 256 native frames per block, at most 200 ms of queued
frames, and 200 ms maximum age by original callback arrival. Slot capacity may be tighter
than 200 ms. Check the controls/epoch and age again before consuming retained or copied data.

Initial application-side AEC bounds to implement and test:

- At most 20 canonical render blocks of 480 frames, with one extra partial block; maximum
  stereo payload is 80,640 bytes, excluding metadata and resampler/native AEC state. Copied
  samples retain their source-generation and oldest contributing arrival time for expiry.
- One partial canonical capture frame; use the primary transport's explicit frame/age cap.
  Do not add a queue that waits indefinitely for a reference.
- Per capture-frame scheduling opportunity, at most 64 native block reads and two 10 ms render
  analysis calls before one capture call. A backlog that needs more work triggers a counted
  discontinuity and fresh alignment; never run an unbounded catch-up loop. Validate this
  initial budget at all supported native rates and callback sizes before enabling AEC.
- These are application buffers, not the AEC's total memory footprint. The inspected native
  code has a 100-frame render transfer queue and configuration-sized filter/delay storage.
  Audit/measure that additional memory and enforce the application cadence; do not treat its
  larger internal queue as permission to accumulate a second of playback latency.
  [Native bounds][native-bounds], [delay buffer][native-delay-buffer].

| Event | Required prototype behavior |
| --- | --- |
| Submitted output underrun silence | Analyze the actual timestamped zeros; do not substitute previously queued speech or reset solely because samples are zero |
| Tap overflow, input trim/drop, gap, epoch change or native xrun | Discard affected application histories/partials, reset AEC and affected resamplers/clock fit, and use the latency-aligned non-AEC microphone path until fresh continuous reference is available |
| Reference missing, expired, association invalid or analysis failure | Report AEC unavailable, continue the independent microphone route without waiting, and never label fallback as echo-protected |
| Incoming Stop/error/recovery | Revoke the old reader immediately; reset learned AEC history before another capture frame can use it; accept only an explicitly associated replacement generation |
| Microphone Stop/restart/recovery | Respect out-of-band cancellation, clear AEC/reference history, and construct fresh worker state; never stop or rebuild the incoming route as a side effect |
| Format or stereo/mono change | Revalidate association and rebuild fixed-format AEC state between generations; preserve output channel order |
| Sustained AEC + DFN deadline pressure | Count total worker cost, preserve microphone priority, enter observable aligned fallback, and keep callbacks independent of inference |

AEC's internal learned/filter history can span more than the external 200 ms queue window.
It must still be reset when the current reference generation is invalidated or expires.
Checking only application-held blocks while retaining invalid learned state is insufficient.

Denoise bypass and AEC bypass are separate decisions. The denoiser's dry branch should use
AEC output while AEC is enabled; a complete AEC fallback needs its own delay alignment.
Measure AEC/filterbank delay with impulses, including any reset/warm-up effects; 10 ms call
size does not establish AEC algorithmic latency. Preserve the existing DFN 10 ms dry alignment
and crossfades. No undocumented change to existing bypass controls is authorized by this design.

### Implementation gates and test plan

1. Add an isolated worker-facing `aec-webrtc` adapter with exact dependencies, reproducible
   native artifacts, notices, format/error validation and borrowed reusable buffers. No CPAL,
   Tauri or DFN dependency belongs inside that adapter. Build on macOS ARM64 and Linux x86_64
   before adding it to the live route; Intel macOS and other hardware remain matrix work.
2. Exercise the native adapter offline with project-owned deterministic fixtures: pure echo
   at several delays, independent stereo references (including anti-phase channels), near-end
   only, double talk, silence, clipping/non-finite rejection and resets. Compare against bypass
   and optionally Speex. Record ERLE on far-end-only intervals and near-end distortion
   separately; do not use double-talk ERLE as proof of quality. Establish numeric thresholds
   from documented fixtures before enabling the prototype, without claiming room acceptance.
3. Implement timestamp-bearing capture and bounded reference handoff/resampling. Test unrelated
   clock origins, 44.1/48/96 kHz, ±100/±750 ppm drift, rate steps, callback jitter, missing times,
   overflow/expiry, Stop/restart/replacement and independent route cancellation. Verify that
   the samples cancelled are exactly the samples passed to DFN.
4. Test AEC before DFN, measured impulse delay and fallback duration/channel order. Exercise
   settings/bypass, model failure, overload, incoming inference suspension and the exact-zero
   underrun reference. Re-run callback allocation tests; measure C++ as well as Rust allocations
   and reset cost. Rust's allocator counter alone cannot establish native allocation behavior.
5. Record AEC-only and combined-worker p50/p95/p99, deadline misses, bounded memory and queue
   occupancy under stereo and CPU pressure. No candidate timing result is inferred from
   upstream examples or the existing 183 Rust tests. E16-05 then supplies physical room,
   double-talk, speaker-volume/path-change and listening acceptance with explicit criteria.

## Validation and remaining work

Source inspection and checksum verification are complete. No candidate audio execution,
local candidate compilation, CPU/ERLE comparison or physical echo test was performed. The
selected build's missing local tools and runtime allocation findings are recorded above.
Repository regression checks on this documentation-only branch:

| Check | Result |
| --- | --- |
| `cargo ci-test --locked` | Passed: 183 tests, 0 failures, 1 pre-existing ignored benchmark |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo ci-fmt` | Passed |
| `npm run --prefix apps/desktop test` | Passed: 46 tests across 8 files |
| `npm run --prefix apps/desktop build` | Passed: TypeScript and Vite production assets |
| `git diff --check` | Passed |
| Documentation/evidence checks | 51 local links resolved; evidence JSON parsed; 13 numbered tasks remain open; 37 source-file hashes recorded and two registry archive checksums verified |

No runtime tests were added for a documentation-only change. These existing-suite results do
not validate AEC. Logs are ignored under `target/aec-evaluation/`: `repository-tests.log`,
`clippy.log`, `frontend-tests.log` and `frontend-build.log`. No desktop launch, candidate build,
native package build or new GitHub CI run is claimed for this evaluation.

Headphones remain the documented fallback. Phase 6's two-hour concurrent hardware runs,
physical device recovery, Bluetooth profile changes and conferencing compatibility remain
open; the short listening test and PR #11's UI smoke test do not satisfy them. The earlier
isolated missed deadline is unchanged. E17 packaging, final notices and supported-hardware
decisions remain open. Next task: **E16-04**, starting with the isolated native build/adapter.

[wap-release]: https://github.com/tonarino/webrtc-audio-processing/releases/tag/v2.1.0
[wap-api]: https://github.com/tonarino/webrtc-audio-processing/blob/c14d7af1760baff83e8210fee336a0cae0faaa7d/src/lib.rs
[wap-config]: https://github.com/tonarino/webrtc-audio-processing/blob/c14d7af1760baff83e8210fee336a0cae0faaa7d/webrtc-audio-processing-config/src/lib.rs
[wap-conversion]: https://github.com/tonarino/webrtc-audio-processing/blob/c14d7af1760baff83e8210fee336a0cae0faaa7d/src/config.rs
[wap-bridge]: https://github.com/tonarino/webrtc-audio-processing/blob/c14d7af1760baff83e8210fee336a0cae0faaa7d/webrtc-audio-processing-sys/src/wrapper.cpp
[wap-build]: https://github.com/tonarino/webrtc-audio-processing/blob/c14d7af1760baff83e8210fee336a0cae0faaa7d/webrtc-audio-processing-sys/build.rs
[wap-ci]: https://github.com/tonarino/webrtc-audio-processing/blob/c14d7af1760baff83e8210fee336a0cae0faaa7d/.github/workflows/rust.yml
[wap-license]: https://github.com/tonarino/webrtc-audio-processing/blob/c14d7af1760baff83e8210fee336a0cae0faaa7d/COPYING
[native-api]: https://gitlab.freedesktop.org/pulseaudio/webrtc-audio-processing/-/blob/d0569cfa50c1858ee279d77b3fc8870be6902441/webrtc/api/audio/audio_processing.h
[native-build]: https://gitlab.freedesktop.org/pulseaudio/webrtc-audio-processing/-/blob/d0569cfa50c1858ee279d77b3fc8870be6902441/meson.build
[native-license]: https://gitlab.freedesktop.org/pulseaudio/webrtc-audio-processing/-/blob/d0569cfa50c1858ee279d77b3fc8870be6902441/webrtc/LICENSE
[native-patents]: https://gitlab.freedesktop.org/pulseaudio/webrtc-audio-processing/-/blob/d0569cfa50c1858ee279d77b3fc8870be6902441/webrtc/PATENTS
[native-bounds]: https://gitlab.freedesktop.org/pulseaudio/webrtc-audio-processing/-/blob/d0569cfa50c1858ee279d77b3fc8870be6902441/webrtc/modules/audio_processing/aec3/aec3_common.h
[native-delay-buffer]: https://gitlab.freedesktop.org/pulseaudio/webrtc-audio-processing/-/blob/d0569cfa50c1858ee279d77b3fc8870be6902441/webrtc/modules/audio_processing/aec3/render_delay_buffer.cc
[abseil-wrap]: https://gitlab.freedesktop.org/pulseaudio/webrtc-audio-processing/-/blob/d0569cfa50c1858ee279d77b3fc8870be6902441/subprojects/abseil-cpp.wrap
[abseil-license]: https://github.com/abseil/abseil-cpp/blob/20240722.0/LICENSE
[rust-port]: https://github.com/RubyBit/aec3-rs/tree/8b6d0bad184eaf5b594adf808fb702b698b411ad
[rust-render]: https://github.com/RubyBit/aec3-rs/blob/8b6d0bad184eaf5b594adf808fb702b698b411ad/src/audio_processing/aec3/echo_canceller3.rs
[rust-graph]: https://github.com/RubyBit/aec3-rs/blob/8b6d0bad184eaf5b594adf808fb702b698b411ad/src/graph/runtime.rs
[rust-license]: https://github.com/RubyBit/aec3-rs/blob/8b6d0bad184eaf5b594adf808fb702b698b411ad/LICENSE
[rust-patent]: https://github.com/RubyBit/aec3-rs/blob/8b6d0bad184eaf5b594adf808fb702b698b411ad/PATENT
[speex-api]: https://github.com/xiph/speexdsp/blob/1b28a0f61bc31162979e1f26f3981fc3637095c8/include/speex/speex_echo.h
[speex-manual]: https://github.com/xiph/speexdsp/blob/1b28a0f61bc31162979e1f26f3981fc3637095c8/doc/manual.lyx
[speex-license]: https://github.com/xiph/speexdsp/blob/1b28a0f61bc31162979e1f26f3981fc3637095c8/COPYING
