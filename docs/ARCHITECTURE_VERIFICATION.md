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
`Rc<Tensor>`, non-`Send` operator state, and non-`Sync` scratch state inside Tract. The offline
example contains a negative compile-time assertion for both traits.

This confirms ADR-004: construct, use, reset or reconstruct, and drop the processor on the same DSP
worker thread. A loaded processor cannot be created by the supervisor and moved into that worker.

### Runtime settings and reset

Verified construction settings are channel count, post-filter state/beta, attenuation limit,
stage thresholds, and multi-channel mask reduction. `DfTract` exposes in-place
`set_pf_beta` and `set_atten_lim` methods. There is no verified generic `strength` control, so no
such public setting should be introduced.

There is no explicit full-model reset API. `DfTract::init` rebuilds rolling spectral and DSP
buffers, but source inspection does not establish that it resets every Tract recurrent operator
state, and it allocates. Until a focused adapter test proves otherwise, a full reset must
reconstruct the processor on its owning worker between frames.

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

Source verification found two capabilities newer than the original architecture assumptions:

- CoreAudio can build an input stream from an output-only device by creating a process-owned
  aggregate/tap on macOS 14.6 or later.
- WASAPI can build an input stream from a render device using the native loopback flag.

Neither path was opened in Phase 0, so permissions, capture contents, cleanup, recursion safety,
and hardware behavior remain unverified. They stay behind the future platform-routing abstraction.
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
