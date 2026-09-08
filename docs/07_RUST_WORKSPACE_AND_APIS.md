# Rust Workspace and Public APIs

## 1. Workspace layout

```text
noise-reduction-app/
├── Cargo.toml
├── rust-toolchain.toml
├── README.md
├── apps/
│   └── desktop/
│       ├── package.json
│       ├── vite.config.ts
│       ├── src/
│       └── src-tauri/
│           ├── Cargo.toml
│           ├── capabilities/
│           └── src/
│               ├── main.rs
│               ├── commands.rs
│               ├── events.rs
│               ├── state.rs
│               └── api_error.rs
├── crates/
│   ├── audio-protocol/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── device.rs
│   │       ├── route.rs
│   │       ├── settings.rs
│   │       ├── metrics.rs
│   │       └── event.rs
│   ├── audio-engine/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── engine.rs
│   │       ├── handle.rs
│   │       ├── supervisor.rs
│   │       ├── pipeline.rs
│   │       ├── state_machine.rs
│   │       ├── command.rs
│   │       └── metrics_store.rs
│   ├── audio-cpal/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── devices.rs
│   │       ├── format.rs
│   │       ├── input.rs
│   │       ├── output.rs
│   │       └── error.rs
│   ├── audio-dsp/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── channel_mapper.rs
│   │       ├── frame_assembler.rs
│   │       ├── resampler.rs
│   │       ├── delay_line.rs
│   │       ├── crossfade.rs
│   │       ├── limiter.rs
│   │       └── meters.rs
│   ├── denoise-core/
│   │   └── src/lib.rs
│   ├── denoise-dfn/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── processor.rs
│   │       ├── model_asset.rs
│   │       ├── settings.rs
│   │       └── error.rs
│   └── platform-routing/
│       └── src/
│           ├── lib.rs
│           ├── macos.rs
│           ├── linux.rs
│           └── pipewire.rs
├── models/
│   ├── manifest.json
│   └── deepfilternet-low-latency.tar.gz
└── tests/
    ├── fixtures/
    ├── realtime-simulation/
    └── hardware/
```

`denoise-core` contains the backend-neutral `NoiseReducer` trait. `denoise-dfn` implements it.

## 2. Dependency rules

```text
audio-protocol: no project-internal dependencies

denoise-core -> audio-protocol only when domain types are shared
audio-dsp -> audio-protocol
audio-cpal -> audio-protocol
denoise-dfn -> denoise-core + audio-protocol
audio-engine -> audio-protocol + audio-cpal + audio-dsp + denoise-core + denoise-dfn + platform-routing
platform-routing -> audio-protocol
Tauri desktop -> audio-engine + audio-protocol
```

Avoid circular dependencies. If shared primitive types create a cycle, move them into `audio-protocol` or a smaller foundational crate.

## 3. Domain types

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RouteId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DeviceId(pub String);

#[derive(Clone, Debug)]
pub struct AudioFormat {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub sample_format: SampleFormat,
    pub buffer_size_frames: Option<u32>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ProcessingMode {
    SpeechDenoise,
    AlignedBypass,
}
```

Use newtypes for identifiers to avoid mixing route, device, and endpoint IDs.

## 4. Engine API

```rust
#[derive(Clone)]
pub struct AudioEngineHandle {
    command_tx: crossbeam_channel::Sender<EngineCommand>,
    snapshot: std::sync::Arc<EngineSnapshotStore>,
}

impl AudioEngineHandle {
    pub fn try_send(&self, command: EngineCommand) -> Result<(), EngineError>;
    pub fn snapshot(&self) -> EngineSnapshot;
}

pub enum EngineCommand {
    RefreshDevices,
    StartRoute(RouteConfig),
    StopRoute(RouteId),
    UpdateDenoise {
        route_id: RouteId,
        settings: DenoiseSettings,
    },
    SetBypass {
        route_id: RouteId,
        bypassed: bool,
    },
    Shutdown,
}
```

The command channel must be bounded. `try_send` should return a typed busy/backpressure error rather than block the Tauri command thread indefinitely.

## 5. Engine events

```rust
pub enum EngineEvent {
    StateChanged {
        route_id: RouteId,
        state: PipelineState,
    },
    DevicesChanged,
    Metrics {
        route_id: RouteId,
        metrics: PipelineMetrics,
    },
    Fault {
        route_id: Option<RouteId>,
        error: EngineErrorDto,
        recoverable: bool,
    },
}
```

The engine may expose events through a bounded receiver or callback registered by the Tauri layer. Do not make Tauri types part of this interface.

## 6. Backend abstractions

### Audio device backend

```rust
pub trait AudioBackend {
    fn list_devices(&self) -> Result<DeviceInventory, AudioIoError>;

    fn build_input_stream(
        &self,
        request: InputStreamRequest,
        callback: InputCallback,
        error_callback: StreamErrorCallback,
    ) -> Result<Box<dyn ManagedStream>, AudioIoError>;

    fn build_output_stream(
        &self,
        request: OutputStreamRequest,
        callback: OutputCallback,
        error_callback: StreamErrorCallback,
    ) -> Result<Box<dyn ManagedStream>, AudioIoError>;
}
```

Use an abstraction only if it materially improves testing. Avoid forcing CPAL callback generics through a large object-safe trait without first prototyping the type design.

### Noise reducer

Defined in `denoise-core` and implemented by `denoise-dfn`.

### Platform router

Defined in `platform-routing`; implementations selected with target-specific modules and feature flags.

## 7. Error hierarchy

Use `thiserror` internally and stable DTOs externally.

```rust
pub enum EngineError {
    InvalidState { route_id: RouteId, state: PipelineState },
    CommandQueueFull,
    DeviceNotFound(DeviceId),
    UnsupportedFormat(String),
    AudioIo(AudioIoError),
    Dsp(DspError),
    Denoise(DenoiseError),
    Routing(RoutingError),
    WorkerTerminated,
    ShutdownInProgress,
}
```

UI-facing errors should include:

```rust
pub struct EngineErrorDto {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
    pub suggested_action: Option<String>,
}
```

Never depend on parsing human-readable error strings for control flow.

## 8. Suggested dependencies

Validate current versions and APIs before use.

| Concern | Candidate |
|---|---|
| Audio I/O | `cpal` |
| SPSC audio rings | `ringbuf` or a purpose-built bounded ring |
| Bounded control channels | `crossbeam-channel` |
| Serialization | `serde`, `serde_json` |
| Errors | `thiserror` |
| Logging | `tracing`, `tracing-subscriber` |
| Atomic snapshots | custom atomics or `arc-swap` outside callbacks |
| Resampling | `rubato` or another preallocatable Rust resampler validated for worker use |
| Hash verification | `sha2` |
| Tauri | `tauri` v2 and required official plugins only |

Do not add an async runtime to the audio data path. Tauri may use async commands for control-plane work, but model inference and callbacks remain on dedicated native threads.

## 9. Feature flags

Possible workspace features:

```toml
[features]
default = ["deepfilternet"]
deepfilternet = []
fake-denoiser = []
platform-loopback = []
virtual-audio = []
realtime-diagnostics = []
```

Keep fake components available for deterministic CI without shipping them accidentally in production builds unless explicitly useful.

## 10. Configuration types

```rust
pub struct RouteConfig {
    pub id: RouteId,
    pub source: SourceEndpoint,
    pub destination: DestinationEndpoint,
    pub processing_mode: ProcessingMode,
    pub target_latency_ms: u16,
    pub denoise: DenoiseSettings,
}
```

Configuration is immutable for a route startup attempt. Runtime changes are explicit commands and produce a new effective settings snapshot.

## 11. Logging

- Use structured `tracing` outside callbacks.
- Assign `route_id` spans to supervisor and worker logs.
- Rate-limit repeated stream errors.
- Keep counters in callbacks and translate them to logs on the metrics/control thread.
- Never log PCM samples.
- Redact or normalize user paths and device details in exported reports when appropriate.

## 12. Coding standards

- Deny or warn on unsafe code at the workspace level unless a narrowly reviewed module requires it.
- Document every unsafe block with invariants.
- Prefer explicit units in names: `_hz`, `_ms`, `_frames`, `_samples`, `_ppm`.
- Do not use `unwrap` or `expect` in production audio lifecycle code.
- Keep allocations visible in constructors.
- Use deterministic shutdown ownership; avoid detached threads.
- Add `#[must_use]` to handles or guards whose drop semantics matter.
- Run `cargo fmt`, `cargo clippy --all-targets --all-features`, and tests in CI.
