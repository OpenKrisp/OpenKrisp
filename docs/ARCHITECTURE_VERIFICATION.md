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
