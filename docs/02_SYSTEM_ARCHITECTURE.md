# System Architecture

## 1. Architectural summary

The application is split into a **control plane** and a **real-time data plane**.

- The **control plane** consists of the Tauri UI, Tauri commands/events, configuration persistence, and an `AudioEngineHandle`.
- The **data plane** consists of CPAL streams, bounded sample queues, resampling, frame assembly, DeepFilterNet inference, post-processing, and metrics.

The control plane may be asynchronous and user-driven. The data plane must be deterministic, bounded, and isolated from WebView behavior.

## 2. Component diagram

```mermaid
flowchart LR
    subgraph ControlPlane[Control plane]
        UI[Tauri WebView]
        CMD[Tauri commands]
        EVT[Tauri events]
        HANDLE[AudioEngineHandle]
        SUP[Audio supervisor]
        CONFIG[Configuration store]
    end

    subgraph DataPlane[Real-time data plane]
        IN[CPAL input callback]
        INQ[(Bounded input SPSC ring)]
        DSP[DSP worker thread]
        OUTQ[(Bounded output SPSC ring)]
        OUT[CPAL output callback]
        METRICS[Atomic metrics snapshot]
    end

    subgraph Platform[Platform routing]
        ROUTER[PlatformAudioRouter]
        VIRTUAL[Virtual/loopback endpoints]
    end

    UI --> CMD
    CMD --> HANDLE
    HANDLE --> SUP
    SUP --> IN
    SUP --> DSP
    SUP --> OUT
    CONFIG <--> CMD

    VIRTUAL <--> ROUTER
    ROUTER --> SUP

    IN --> INQ
    INQ --> DSP
    DSP --> OUTQ
    OUTQ --> OUT
    DSP --> METRICS
    IN --> METRICS
    OUT --> METRICS
    METRICS --> HANDLE
    HANDLE --> EVT
    EVT --> UI
```

## 3. Logical components

| Component | Responsibility | Explicit exclusions |
|---|---|---|
| Tauri WebView | Device selection, controls, meters, diagnostics, profiles | PCM processing, audio timing |
| Tauri command layer | Validate DTOs and forward commands | Owning CPAL streams or model state |
| `AudioEngineHandle` | Thread-safe command submission and status reads | Blocking until audio work finishes |
| Audio supervisor | Route lifecycle, device refresh, recovery, thread ownership | Per-sample DSP |
| CPAL adapter | Device discovery, format negotiation, stream construction | DeepFilterNet inference |
| Input callback | Convert samples and enqueue them | Blocking, logging, allocations, inference |
| DSP worker | Resample, map channels, frame model input, infer, post-process | WebView access |
| Output callback | Dequeue and write samples or fallback silence | Waiting on inference |
| DeepFilterNet adapter | Model loading, state, frame processing, reset | Device discovery and UI concerns |
| DSP utilities | Resampling, delay, crossfade, limiter, meters | Application lifecycle |
| Platform router | Virtual endpoints, loopback selection, route safety | Model inference |
| Metrics store | Lock-free or low-contention snapshots | High-frequency UI emission |

## 4. Route model

A **route** is one independently managed audio graph:

```text
Source endpoint
  -> source format conversion
  -> input queue
  -> canonical-rate processing graph
  -> output queue
  -> destination format conversion
  -> destination endpoint
```

Examples:

- `mic-monitor-1`: physical microphone to headphones.
- `virtual-mic-1`: physical microphone to virtual microphone endpoint.
- `incoming-speech-1`: loopback input to physical headphones.

Each route owns:

- CPAL input stream, when applicable.
- CPAL output stream, when applicable.
- Input and output rings.
- One DSP worker thread.
- One DeepFilterNet processor instance.
- Resampler state.
- Delayed dry/bypass state.
- Metrics and fault state.
- A route-specific cancellation signal.

Sharing a physical device across routes may be added later, but the first implementation should favor route isolation over premature graph sharing.

## 5. Data-plane flow

### 5.1 Microphone route

```mermaid
flowchart LR
    A[Physical microphone] --> B[CPAL input callback]
    B --> C[(Input ring)]
    C --> D[Input channel mapping]
    D --> E[Resample to model rate]
    E --> F[Frame assembler]
    F --> G[DeepFilterNet]
    E --> H[Latency-matched dry delay]
    G --> I[Wet/dry crossfade]
    H --> I
    I --> J[Limiter and meters]
    J --> K[Resample to output rate]
    K --> L[Output channel mapping]
    L --> M[(Output ring)]
    M --> N[CPAL output callback]
    N --> O[Headphones or virtual endpoint]
```

### 5.2 Speaker-speech route

The same graph can be reused with a loopback or application-owned source. An explicit processing-mode gate must allow non-speech audio to bypass DeepFilterNet.

## 6. Control-plane flow

### Starting a route

1. The UI sends a validated `StartRoute` request.
2. The Tauri command forwards it to `AudioEngineHandle` through a bounded command channel.
3. The supervisor resolves endpoints and negotiates formats.
4. The supervisor allocates rings, DSP buffers, and metrics.
5. The DSP worker thread starts and constructs DeepFilterNet locally.
6. The worker warms the model with silence and reports readiness.
7. The supervisor creates the output stream, pre-fills the output ring, then creates the input stream.
8. Streams are started in a documented order.
9. The route transitions to `Running` and a state event is emitted.

### Stopping a route

1. The supervisor transitions the route to `Stopping`.
2. Input production stops.
3. The worker is signaled to drain or terminate according to shutdown mode.
4. Output is faded to silence.
5. CPAL streams are dropped on their owning lifecycle context.
6. The worker thread is joined.
7. Buffers and model state are released.
8. The route transitions to `Stopped`.

## 7. State machine

```mermaid
stateDiagram-v2
    [*] --> Stopped
    Stopped --> Starting: StartRoute
    Starting --> Running: Streams and model ready
    Starting --> Failed: Initialization failed

    Running --> Reconfiguring: Device/format change
    Reconfiguring --> Running: Rebuild succeeded
    Reconfiguring --> Recovering: Rebuild failed temporarily

    Running --> Recovering: Device loss/xrun/worker fault
    Recovering --> Running: Recovery succeeded
    Recovering --> Failed: Retry budget exhausted

    Running --> Stopping: StopRoute
    Failed --> Starting: Retry
    Failed --> Stopping: StopRoute
    Stopping --> Stopped: Resources released
```

## 8. Dependency direction

Dependencies must point inward toward reusable core abstractions:

```text
Tauri desktop
    -> audio-engine
        -> audio-cpal
        -> audio-dsp
        -> denoise-dfn
        -> platform-routing
        -> audio-protocol

All crates -> audio-protocol where shared DTO/domain types are needed
```

Forbidden dependencies:

- `audio-engine` must not depend on Tauri.
- `denoise-dfn` must not depend on CPAL.
- `audio-dsp` must not depend on Tauri, CPAL, or DeepFilterNet.
- CPAL callbacks must not call Tauri APIs.
- Platform-routing implementations must not own model state.

## 9. Error boundaries

Errors are handled at the closest safe boundary:

- **Sample conversion issue:** count and substitute a safe sample if possible.
- **Ring overrun/underrun:** update counters and apply the route policy without blocking.
- **Model frame error:** reset or transition to aligned bypass; notify supervisor.
- **Stream error:** send a compact error message to the supervisor.
- **Device loss:** rebuild or fail the route.
- **Tauri command validation error:** reject before modifying engine state.
- **Worker panic:** detect through thread completion or a panic boundary, mark failed, and release the route.

## 10. Architectural priorities

When tradeoffs arise, apply this order:

1. No crashes, deadlocks, or unbounded latency.
2. Real-time callback safety.
3. Stable audio continuity.
4. Correct routing and lifecycle cleanup.
5. Predictable latency.
6. Noise-reduction quality.
7. CPU efficiency.
8. UI richness.

A temporary aligned bypass is preferable to stalled callbacks or ever-growing queues.
