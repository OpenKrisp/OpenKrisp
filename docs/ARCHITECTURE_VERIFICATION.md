# Architecture Verification

## 1. Scope and environment

This report records roadmap Phase 0 findings as of 2026-09-02. It verifies dependency and model
facts needed by later public APIs; it does not implement a live audio route.

Primary development environment:

| Item | Verified value |
|---|---|
| Operating system | macOS 26.6.2, Darwin 25.6.0 |
| Architecture | Apple Silicon (`aarch64-apple-darwin`) |
| Rust | 1.98.0, pinned with Rustfmt and Clippy |
| Node.js | 24.12.0 |
| Default CPAL host | CoreAudio |

The Rust workspace builds without CMake or `pkg-config` on this macOS environment. Linux CI still
installs ALSA development headers and `pkg-config`, which CPAL's ALSA backend requires.

## 2. Pinned dependencies and assets

| Component | Pin | Notes |
|---|---|---|
| CPAL | crates.io `0.18.2` | Exact version; checksum is locked in `Cargo.lock` |
| DeepFilterNet | tag `v0.5.6`, commit `978576aa8400552a4ce9730838c635aa30db5e61` | Git revision is exact; package name is `deep_filter`, Rust import name is `df` |
| DeepFilterNet inference | `tract` feature, default features disabled | Avoids compiling an embedded default model; the application supplies the asset |
| Tract | `0.19.16` | Resolved and locked from DeepFilterNet's `^0.19.4` requirement |
| Low-latency model | `models/DeepFilterNet3_ll_onnx.tar.gz` | Official asset from the pinned revision |
| Model SHA-256 | `5998e58e8ba0e09bb76986ef97b84afa065a571ef282d4a1222f341e3251cf3a` | Checked before every offline spike run |

`Cargo.lock` is part of the reproducibility boundary. DeepFilterNet's 2023 manifest permits newer
compatible transitive versions, and resolving it without the lock can materially change the build.
Rust 1.98.0 is used because it is the latest stable toolchain in the primary environment and
supports the currently resolved graph.

## 3. DeepFilterNet API and model metadata

The verified loading and processing sequence is:

```rust
use df::tract::{DfParams, DfTract, RuntimeParams};

let params = DfParams::new(model_path)?;
let runtime = RuntimeParams::default_with_ch(1);
let mut model = DfTract::new(params, &runtime)?;
let local_snr_db = model.process(input.view(), output.view_mut())?;
```

The selected model reports:

| Property | Verified value | Consequence |
|---|---:|---|
| Sample rate | 48,000 Hz | Device-native audio must be resampled outside the model adapter |
| Processing layout | Planar `f32`, ndarray shape `[channels, samples]` | The engine-to-adapter boundary must not pass interleaved buffers directly |
| Validated route layout | Mono | Phase 0 exercised `RuntimeParams::default_with_ch(1)`; multi-channel quality remains unverified |
| Hop/model-call size | 480 samples per channel | One call advances the timeline by 10 ms |
| FFT window | 960 samples | The real-time STFT contributes one-hop delay |
| Model lookahead | 0 hops | This is the low-latency asset, not the normal DFN3 asset |
| Algorithmic latency | 480 samples / 10 ms | `fft_size - hop_size + lookahead * hop_size` |
| Output timeline | 480 samples per channel per successful call | Same duration and planar shape as input |
| Return statistic | Local SNR estimate in dB | It is not a speech probability or attenuation measurement |

The offline spike loaded the packaged asset and produced finite, correctly sized output for silence
and a deterministic synthetic noisy frame. Silence below the upstream RMS threshold takes a fast
zero-output path; the non-silent fixture exercises Tract inference.

### Thread traits

At the pinned revision, `DfTract` is neither `Send` nor `Sync`. Compiler diagnostics identify
`Rc<Tensor>`, non-`Send` operator state, and non-`Sync` scratch state inside Tract. The production
adapter contains a negative compile-time assertion for both traits.

This confirms ADR-004: construct, use, reset or reconstruct, and drop the processor on the same DSP
worker thread. A loaded processor cannot be created by the supervisor and moved into that worker.

### Runtime settings and reset

Verified construction settings are channel count, post-filter state/beta, attenuation limit,
stage thresholds, and multi-channel mask reduction. `DfTract` exposes in-place
`set_pf_beta` and `set_atten_lim` methods. There is no verified generic `strength` control, so no
such public setting should be introduced.

There is no explicit full-model reset API. `DfTract::init` rebuilds rolling spectral and DSP
buffers, but source inspection does not establish that it resets every Tract recurrent operator
state, and it allocates. The phases 0–4 review fix now saves a pristine `DfTract` clone at
construction and restores from it between frames. The pinned clone shares immutable prepared
plans and clones temporal state; current runtime settings are reapplied. A production-adapter
regression compares three repeated non-silent reset cycles against a fresh processor, with non-zero
reference output and a 1e-6 sample tolerance. This avoids graph rebuilding without relying on
`DfTract::init`. Reset still allocates/copies bounded model state on the DSP worker; it is not a
callback-safe operation. See `PHASE_0_4_REVIEW_FIXES.md` for measured recovery evidence.

### Allocation and validation constraints

The upstream `process` method accepts borrowed caller buffers, but its implementation clones
tensors, materializes an owned spectrum during synthesis, and receives newly produced model output
tensors. It is therefore not verified allocation-free per model frame. This work remains on the DSP
worker and never belongs in a CPAL callback, but Phase 1/adapter allocation measurements may require
an upstream patch, a maintained fork, or a different supported revision.

Dimension checks in upstream `process` are debug assertions, and internal code contains infallible
assumptions. The production adapter must validate exact dimensions and finite samples before entry;
it must not expose arbitrary ndarray shapes from engine code.

## 4. CPAL findings

### Device inventory

The `audio-cpal` example successfully enumerated five local CoreAudio endpoints. It identified
default input/output roles, host-qualified device IDs, structured descriptions, and default stream
formats. The observed hardware included mono 48 kHz `f32` input and stereo 44.1/48 kHz `f32`
output, validating the need for explicit channel mapping and sample-rate conversion.

CPAL 0.18.2 provides `DeviceTrait::id`, `Display`/`FromStr` for `DeviceId`, and
`HostTrait::device_by_id`. Its documentation says IDs are stable across program runs, disconnects,
and reboots where the backend can provide that guarantee. Profiles must still retain fallback
behavior because stability is not unconditional.

### Sample and error types

`SampleFormat` is non-exhaustive and currently includes signed and unsigned 8/16/24/32/64-bit PCM,
`f32`, `f64`, and DSD-in-8/16/32-bit-container variants. Format conversion code must use a wildcard
for future or unsupported formats.

CPAL 0.18.2 uses one `cpal::Error` carrying a non-exhaustive `ErrorKind`, rather than the older
operation-specific public error types assumed by some examples. Relevant kinds include device or
host loss, default-device change, permission denial, stream invalidation, xrun, unsupported format,
and backend failure. Stream error callbacks receive this error without requiring formatted text;
later callback code should forward only the compact kind/counter to the supervisor.

### Loopback capability

Source verification found a CoreAudio capability newer than the original architecture assumptions:

- CoreAudio can build an input stream from an output-only device by creating a process-owned
  aggregate/tap on macOS 14.6 or later.

That path was not opened in Phase 0, so permissions, capture contents, cleanup, recursion safety,
and hardware behavior remain unverified. It stays behind the future platform-routing abstraction.
The default Linux build uses ALSA; general system loopback is not established by this spike, while
CPAL's PipeWire/PulseAudio/JACK backends are optional features.

## 5. License verification

The locked Cargo graph contains 210 external packages across all target-specific dependencies. All
declare SPDX-compatible permissive choices covered by the repository's `cargo-deny` policy:
Apache-2.0, MIT, CC0-1.0, Unicode-3.0, or Zlib. No required copyleft dependency was found. CPAL is
Apache-2.0; DeepFilterNet/libDF is MIT or Apache-2.0; TypeScript and its platform packages are
Apache-2.0.

The DeepFilterNet repository states that its contents are dual-licensed MIT or Apache-2.0 and does
not identify a separate license for the selected model archive. Exact upstream license files are
stored beside the model. Before a public binary release, legal review should confirm that the
repository-level grant is sufficient for model redistribution and generate complete notices from
the final packaged dependency graph.

## 6. Verified commands

```text
cargo check --workspace --all-targets --all-features
cargo run --locked -p audio-cpal --example list_devices
cargo run --locked -p denoise-dfn --example offline_inference
cargo metadata --format-version 1 --locked
npm --prefix apps/desktop run typecheck
```

The complete Phase 0 gate also runs formatting, Clippy with warnings denied, workspace tests, and
the `cargo-deny` license check. Hardware streams, loopback capture, callback real-time behavior,
resampling, and multi-hour stability are intentionally not claimed by this phase.


## Named virtual devices (2026-09-18)

The current macOS installer builds separate OpenKrisp Microphone and OpenKrisp Speaker
HAL plugins from pinned BlackHole source. Exact native UIDs, roles, virtual transport
and channel widths remain mandatory; public/mirror ancestry stays shared within each
bus. Only the two OpenKrisp endpoints are supported; old BlackHole selections are rejected. The Linux incoming sink
now displays OpenKrisp Speaker while retaining its node IDs. See the
[naming implementation and validation](VIRTUAL_DEVICE_NAMES.md). No engine callback,
PCM IPC, AEC reference or model ownership change is involved. Linux hardware/conferencing
remains untested, and the earlier macOS call report does not qualify the new driver builds.

## Phase 5: Existing virtual endpoint and native metadata

The primary integration is the standard user-installed BlackHole 2ch on macOS. Logical virtual
endpoint `macos:blackhole-2ch` resolves to CPAL `coreaudio:BlackHole2ch_UID`; the optional mirror
UID shares its recursive ancestry. The adapter is stateless: CPAL owns streams and cleanup, while
OpenKrisp never modifies system defaults or installs the driver. Unknown and aggregate microphone
ancestry is rejected for virtual routes. See the [Phase 5 report](PHASE_5_VIRTUAL_MICROPHONE_IMPLEMENTATION_REPORT.md)
for source references, tests and unverified receiving-app acceptance.

CPAL 0.18.2 does not populate the normal CoreAudio transport type, although it detects aggregates.
`audio-cpal` now reads UID-to-device and transport properties through a macOS-only metadata module.
This adds a narrowly scoped unsafe-code allowance for two checked FFI calls; the rest of the crate
retains `deny(unsafe_code)`. No native handles or audio enter IPC, and no metadata query runs on an
audio callback. The objc2 versions remain pinned to the already-locked 0.3.2 dependencies.


## macOS and Linux virtual microphones (2026-09-08)

The Phase 5 extension enables CPAL 0.18.2's native PipeWire feature on Linux. Its default host
selects PipeWire when available, otherwise ALSA. PipeWire's library and tools are runtime
prerequisites. Linux now uses the dedicated `platform-routing::linux::LinuxRouter`; macOS retains
BlackHole. Windows support and its implementation have been removed; see
[the removal decision](WINDOWS_SUPPORT.md) and [implementation report](PHASE_5_CROSS_PLATFORM_REPORT.md).

Linux uses native CPAL bus metadata, with sysfs-backed ALSA address verification where a PipeWire
node omits its parent bus; unknown devices fail closed. PipeWire sink-monitor inputs are excluded,
and the app's fixed source/sink IDs classify as virtual.

After removal, native Linux audio/router tests and a live private PipeWire cable test pass in an
ARM64 Debian 12 container with PipeWire 0.3.65/WirePlumber 0.4.13. The Linux audio/router/engine crates
also pass warnings-denied Clippy including all targets. Physical microphone, desktop packaging,
receiving-app and long-run acceptance remains required on macOS and Linux.

### Review follow-up: fixed device identities and process lifetime

The pinned CPAL PipeWire implementation represents `input_default`/`output_default` as aliases
that follow default metadata. They are not fixed node identities. Both UI selection and backend
route validation now reject their full `pipewire:` IDs, even if classification changes from
Unknown to Physical. Concrete device IDs still use fresh inventory validation during recovery.

Linux loopback spawning uses a narrowly scoped unsafe module with pinned `libc` 0.2.189
(already present in the lockfile). Its `CommandExt::pre_exec` hook sets
`PR_SET_PDEATHSIG(SIGKILL)` and verifies `getppid()` against the captured parent PID to close
the pre-arming process-exit race. The hook uses only syscalls and raw OS errors: no allocation,
locks, logging, environment access or audio work. `platform-routing` now uses `deny(unsafe_code)`
with an allowance exclusively for this module, following the native metadata boundary pattern.

Linux defines the parent as the spawning thread. Production sessions are created and retained
on the same dedicated route owner thread, which outlives the loopback. The setting survives
ordinary exec; OpenKrisp launches the standard unprivileged `pw-loopback` without credential
changes. Drop still kills/reaps the owned child on normal Stop/Quit. Subprocess exit/SIGTERM/
SIGKILL regressions and real private PipeWire node-removal/recreation tests passed on Debian 12.
This protects newly created clients; it does not claim ownership of endpoints left by older
versions or external software.

References: [Linux parent-death signal semantics](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html),
[Rust pre-exec safety contract](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.pre_exec).

## Phase 6 incoming audio (2026-09-08)

The locked model is still mono: 48 kHz, 480-sample frames, 480-sample algorithmic delay.
Incoming stereo therefore composes two independent mono processors on one DSP worker and uses
preallocated planar conversion at the model boundary. The production voiced fixture verifies
independent histories; the worker fixture verifies channel ordering and sample-rate duration.
No upstream multichannel-model capability is assumed. ADR-003 and ADR-011 record this policy and
the two-slot supervisor/microphone priority changes.

Dedicated loopback capture is implemented using the existing BlackHole endpoint and a separate
Linux PipeWire sink/source pair. The private Debian 12 PipeWire test passes simultaneous distinct
stereo transfer and independent incoming cleanup while the virtual microphone remains active.
The same BlackHole 2ch path is rejected for concurrent virtual microphone and incoming use.
Native application PCM uses a move-only bounded Rust source, with no serializable PCM API.

See [the Phase 6 report](PHASE_6_INCOMING_SPEECH_IMPLEMENTATION_REPORT.md) for exact APIs,
verification commands, upstream routing references and the unverified hardware acceptance boundary.


## Desktop settings, profiles and diagnostics (2026-09-10)

The desktop completion change exposes the already-verified denoise controls through existing
metadata commands, with snapshot confirmation at f32 precision and independent UI drafts.
Profile schema 2 stores explicit route roles and mode; earlier schemas migrate without an
implicit file rewrite. Loading a profile does not start audio or change a running route.
Diagnostics use existing snapshots and an explicit export allowlist. No native audio callback,
model ownership, IPC command/capability or PCM boundary changes. See the
[implementation report](DESKTOP_SETTINGS_PROFILES_DIAGNOSTICS_REPORT.md) for behavior and evidence.

## Phase 7 render reference and clocks (2026-09-09)

CPAL 0.18.2's locked source and [StreamInstant documentation](https://docs.rs/cpal/0.18.2/cpal/struct.StreamInstant.html)
confirm that cross-stream timestamp subtraction is not generally valid. CoreAudio, PipeWire and
ALSA supply capture/predicted playback timestamps with backend-specific latency estimates and
fallbacks; there is no common exposed hardware-accuracy flag. The adapter maps each same-stream
capture/playback-minus-callback offset onto one process monotonic callback-arrival clock and
labels the result as an estimate. It preserves native time, frame position and stream/epoch ID.

The render tap is verified after device sample conversion; it includes quantization and safe-fill
silence. Capture and render observation queues are independent of the existing DSP rings. Bounded
native readers, clock fitting, delay tracking and lifecycle invalidation are implemented without
PCM IPC, model movement or callback allocation in the tested paths. ADR-012 and
[the implementation report](PHASE_7_RENDER_REFERENCE_IMPLEMENTATION_REPORT.md) define units,
limits, discontinuities and validation. Hardware timing accuracy, acoustic latency and AEC
acceptance remain unverified; Phase 6's outstanding hardware matrix is unchanged.

## AEC candidate verification (2026-09-10)

E16-03 selects `webrtc-audio-processing` 2.1.0's full WebRTC AEC3 as the first prototype
candidate. Its published wrapper/sys archives were SHA-256 checked against the official registry
index. Source inspection verified the 48 kHz / 480-frame planar API, explicit multichannel-render
configuration, internal delay-estimation option, per-call Rust pointer-vector allocation and
native locking/reset behavior. Native Meson/Abseil artifacts and notices are outside Cargo's full
reproducibility/license boundary. No candidate was compiled, benchmarked or integrated.

The primary microphone sample ring does not carry timing metadata; its independently observed
capture tap cannot be assumed to contain the samples retained after worker stale-input trimming.
E16-04 needs timestamp-bearing primary capture before AEC, bounded owner-to-worker reference
handoff, separate reference clock compensation and invalidation of AEC history on discontinuity.
The CPAL I/O delay estimate is not WebRTC's analysis-to-processing stream delay or measured
end-to-end latency. ADR-013 and [the evaluation](AEC_EVALUATION.md) record the selection, source
comparison, license findings, prototype bounds and acceptance gates. Runtime architecture and
all previous hardware acceptance limits are unchanged by this documentation change.

## Native AEC prototype verification (2026-09-10)

E16-04 subsequently built the pinned native APM on macOS ARM64 and Linux x86_64 and integrated
it behind `audio-engine/aec-prototype` with explicit headless association. The bridge borrows
fixed planes and keeps C++ ownership on the DFN worker. Desktop routes remain disabled; PCM
does not cross IPC. Primary timed capture prevents separate PCM and observer queues from
drifting apart after loss. Native-indexed reference resampling preserves stereo and original
arrival-age checks, with owner-only generation handoff/return and immediate learned-state
revocation on invalid reference.

ADR-014 records the history-layout change and isolated FFI boundary. Tests verify exact AEC
output at the DFN input, including denoise bypass; initial capture and post-xrun blocks do not
cause duplicate DFN resets. Native fixtures measured 430 samples of impulse-peak delay, which
is included in the estimated software delay, not presented as measured device/acoustic latency.

The C++ probe found processing allocations; Rust's zero-allocation fixture does not cover them.
Harmonic double talk passes its gain/correlation targets, but stationary broadband double talk
fails and is retained as an opt-in acceptance regression. Native worker heap accounting and
room/hardware acceptance remain incomplete. Actual check counts and benchmark results are in
the [prototype report](AEC_PROTOTYPE_IMPLEMENTATION_REPORT.md) and linked evidence manifest.

## AEC explicit-channel verification (2026-09-11)

The [E16-05 investigation](AEC_QUALITY_INVESTIGATION.md) found that AEC3's own content
detector can indefinitely downmix intermittent stereo despite APM multichannel input.
The native factory now preserves the declared channel count from construction using
upstream mono/stereo defaults, with only automatic content detection disabled. It fixes
the opposite-phase speech-like regression (0.852 → 26.134 dB echo-only ERLE on macOS).
The shared native boundary, thread ownership, bounded queues and PCM IPC prohibition remain.

Current native fixtures and suppression diagnostics ran on macOS ARM64 and Linux x86_64.
Three acceptance gates remain negative: stationary broadband preservation, weaker local
voiced/unvoiced preservation, and independent-stereo delay/gain-step recovery. Experimental
suppression tuning is confined to a standalone diagnostic and is not adopted. The current
post-warm-up C++ new/new[] probe observed zero processing calls, but construction/reset still
allocate and C allocation functions remain unmeasured. This updates the earlier observation
whose processing window included automatic stereo reconfiguration.

Required workspace validation passed (200 Rust tests, 5 explicitly ignored; 46 frontend
tests, Clippy, formatting and build). The paced graph diagnostic reproduced six >10 ms
processing misses without injected load, versus zero with two load threads; this does not
resolve the earlier live miss. The hardware smoke test predates this native fix. Desktop
AEC, E16-05/E16-06 quality acceptance and all earlier hardware gates remain open.

## Speakerphone acceptance verification (2026-09-14/15)

The [speakerphone contract](AEC_SPEAKERPHONE_ACCEPTANCE.md) makes passing software,
listening, timing and actual conferencing gates a prerequisite for desktop AEC
enablement. A separate native quality runner includes the ignored acceptance tests
and rejects missing or skipped tests. Both macOS ARM64 and Linux x86_64 runs report
nine passed, three failed, none ignored; the normal macOS workspace run reports
201 passed, five ignored.
The latter is not a quality acceptance result.

Separating gain-only and delay-only fixtures narrowed the stereo path-change
failure to delay tracking: the combined case retains the old software buffer-delay
estimate and ineffective linear cancellation. See the
[diagnostic results](AEC_QUALITY_INVESTIGATION.md). The standalone probe drains
APM's capacity-one statistics queue from startup to avoid reading stale startup
metrics. Production ownership, callback bounds, PCM transport, resampling,
reference invalidation and AEC configuration are unchanged in this follow-up.

## AEC quality-fix verification (2026-09-15)

The subsequent [quality fixes](AEC_QUALITY_FIXES.md) resolve the three synthetic
failures above with unchanged gain/correlation/ERLE targets. A shared native profile
adjusts delay matching and local-speech preservation; a checked native source patch
adds a stateless stereo delay-sidechain helper. Its fixed 64-sample loops use existing
buffers and stack scalars. Native mixer contracts verify destructive-interference
fallback, silence and unchanged original render planes. The adaptive echo filters
still receive both original channels. PCM transport, callbacks, route ownership,
clock fitting, reference expiry and worker-confined AEC/DFN lifetimes are unchanged.

Fifteen native tests and seven compiled mixer contracts pass on macOS ARM64 and
Linux x86_64. Expanded controls cover early local-only speech with unrelated render,
varied pitch/seeds, reflected/distorted echo, unequal speaker coupling and delay steps
in both directions. Twenty additional early-echo windows were subsequently added and
verified on macOS; the retained Linux hashes describe its earlier test-only snapshot.
The three original gates now run in routine CI. Workspace validation passes 209 Rust
tests (two timing benchmarks ignored), 46 frontend tests, 11 Python tests, Clippy,
formatting, frontend production build and diff checks.

Fresh paced AEC + DFN runs have zero deadline misses with and without two CPU load
threads. Earlier misses remain unexplained. Post-warm-up native C++ `new`/`new[]`
instrumentation observes zero processing allocations; construction/reset and C
allocation coverage remain separate. The optional native listening harness can save
a bounded local processed-output WAV only with an explicit flag, writing after route
shutdown. No new recording, GUI flow, room, conferencing or long-run hardware acceptance
is claimed. E16-05/E16-06 and Phase 6 hardware gates remain open. See the
[current evidence manifest](evidence/aec-quality-fixes-2026-09-15.json) for exact scope.

## Desktop default AEC verification (2026-09-15)

The user subsequently requested desktop default enablement. The desktop now compiles
the native implementation and defaults microphone requests/profiles to enabled;
incoming routes remain excluded. The production owner automatically acquires the
incoming slot's sole render reader, independently of route ID/start order. Generation
identity checks prevent stale returns from replacing a new reader. Deterministic
handoff tests reach actual native AEC processing, revoke learned state on incoming
Stop, and resume after replacement. Existing capture/render clock and lifecycle
regressions, independent DFN ownership and callback allocation bounds still pass.

Only preference/status/counters cross IPC; PCM remains native. Real Tauri command
tests verify default-on, opt-out, metadata serialization and PCM rejection. Frontend
tests distinguish an enabled preference from active cancellation and suppress active
status on route loss/recovery. Noise-reduction bypass leaves AEC enabled. Profile
schema 3 preserves explicit off and migrates missing preferences without rewriting
files on load. Invalid references keep aligned microphone/DFN audio with explicit
unprotected status. This is now the defined desktop fallback, not an acoustic guarantee.

Validation: 216 Rust tests pass (two timing benchmarks ignored), 49 frontend tests,
11 Python tests, Clippy, formatting, dedicated native quality check, feature-disabled
engine compilation and release desktop app build pass. Packaged native notices match
the source copies. [Implementation and evidence](AEC_DESKTOP_DEFAULT_IMPLEMENTATION.md)
record the exact scope. Default enablement supersedes the earlier product setting;
physical speakerphone qualification, separate macOS virtual buses and all earlier
hardware/conferencing gates remain open.

## Separate macOS call buses

The [two-device implementation](MACOS_DUAL_AUDIO_DEVICES.md) extends macOS routing with the
standard BlackHole 16ch incoming bus, leaving the existing BlackHole 2ch virtual microphone.
The pinned upstream 0.7.1 driver/installer sources assign separate identities and buffers to
channel variants; each variant's optional mirror shares its own buffer and is not a second bus.
The official 16ch package matches SHA-256
`57254e2f76cd40db7f3f715238b1a2cb2bd08819d38abf4087f2944f71a3641a` and passed local macOS
signature/notarization assessment. The combined package retains independent repair/upgrade
choices and the original upstream payloads/scripts.

CPAL 0.18.2's CoreAudio input builder explicitly requests interleaved PCM, using the requested
channel count in its stream format and dividing callback sample length by native channels
for frame counts. OpenKrisp therefore negotiates exactly 16 native channels, then selects
channels 1–2 at that boundary. The engine, capture observation tap and DFN still see stereo;
no arbitrary channel-map control is exposed through IPC. Frame timestamps preserve the native
capture instants and the AEC reference remains the actual physical render output. Ordinary
mono/stereo callbacks retain their original contiguous conversion path.

Exact-UID/format validation and selection are repeated by the owner on recovery. New routing,
callback, supervisor, UI/profile and installer regressions exercise the supported graph. The
software result does not replace the missing physical two-device, actual-call or two-hour
acceptance evidence. The development host had only BlackHole 2ch installed during this work.
