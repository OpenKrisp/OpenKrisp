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
        OWNER[Route lifecycle owner]
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
    SUP --> OWNER
    OWNER --> IN
    OWNER --> DSP
    OWNER --> OUT
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
| Audio supervisor | Bounded commands, device refresh, authoritative snapshots, route-owner cancellation | Blocking on route startup/recovery, per-sample DSP |
| Route lifecycle owner | CPAL construction/destruction, activation, cancellable recovery, DSP worker ownership | WebView access, owning model state |
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
- Independent DeepFilterNet state (one processor per incoming mono/stereo channel).
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
    I --> J[Pre-resample non-finite protection]
    J --> K[Resample to output rate]
    K --> SAFETY[Final limiter and output meters]
    SAFETY --> L[Output channel mapping]
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
3. The supervisor publishes `Starting` and starts a route-owner thread.
4. The owner resolves the input, allocates bounded transport, and activates input with PCM gated.
5. Output is negotiated after input activation, preserving Bluetooth duplex profile settling.
6. The owner prepares and activates output, keeping it gated while the DSP worker constructs the model.
7. The worker warms the model with quiet non-zero frames, restores pristine temporal state, and reports readiness.
8. After a cancellation check, PCM gates open and stream activation is verified.
9. The supervisor publishes the resulting snapshot before emitting revisioned observations.

The supervisor remains available throughout startup and recovery. CPAL handles are constructed,
used and dropped on the owner; model construction, reset and destruction remain on the DSP worker.
Native calls already in progress cannot be forcibly interrupted. Retry/activation waits check
cancellation in slices of at most 5 ms, and cancelled generations cannot republish `Running`.

### Stopping a route

1. The supervisor publishes `Stopping` and signals the route owner's out-of-band cancellation flag.
2. Callbacks gate capture and render silence; no new processing is enabled after cancellation.
3. The owner leaves any cancellable startup/recovery wait and drops the input stream.
4. The worker is cancelled and joined; model state is destroyed on that worker.
5. The owner releases output and remaining native resources, then publishes `Stopped`.
6. The supervisor retains one terminal snapshot and joins the finished owner.

Application quit performs joins on a background control-plane task while the Tauri event loop
remains alive. Accepted profile writes drain before exit. Current stop uses immediate silence;
an audible fade-out is not claimed by this implementation.

## 7. State machine

```mermaid
stateDiagram-v2
    [*] --> Stopped
    Stopped --> Starting: StartRoute
    Starting --> Running: Streams and model ready
    Starting --> Failed: Initialization failed
    Starting --> Stopping: StopRoute

    Running --> Reconfiguring: Device/format change
    Reconfiguring --> Running: Rebuild succeeded
    Reconfiguring --> Recovering: Rebuild failed temporarily

    Running --> Recovering: Device loss/xrun/worker fault
    Recovering --> Running: Recovery succeeded
    Recovering --> Failed: Retry budget exhausted
    Recovering --> Stopping: StopRoute
    Reconfiguring --> Stopping: StopRoute

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

## Phase 6 route concurrency

The supervisor now retains one microphone and one incoming route, each with its own owner and
DSP worker. Incoming stereo keeps independent channel histories; user bypass and microphone
pressure can suspend incoming inference while the aligned dry graph continues. Quit cancels both
owners before joining. Native application sources use bounded Rust rings and never enter IPC.
See [the implementation report](PHASE_6_INCOMING_SPEECH_IMPLEMENTATION_REPORT.md).

macOS uses distinct BlackHole-based devices for simultaneous call routing:
OpenKrisp Microphone (2ch) and OpenKrisp Speaker (16ch, capturing channels 1–2).
These are the only supported macOS virtual routes; old BlackHole endpoint IDs are rejected.
See the [native naming and installer contract](VIRTUAL_DEVICE_NAMES.md).
Platform preparation selects the native capture width after identity/ancestry validation;
CPAL delivers a bounded stereo selection before the input ring and capture tap. The render
reference remains at physical output after incoming processing. No channel-selection
request or PCM crosses IPC. See [the two-device design](MACOS_DUAL_AUDIO_DEVICES.md).


## Phase 7 native audio observations

CPAL input conversion and final device-output conversion now publish separate bounded native
capture/render taps. The render reference represents submitted device PCM, including actual
underrun silence, after all worker processing and channel/rate conversion. The microphone and
incoming routes retain separate owners and models; a two-slot native registry transfers each
stream generation's sole reader outside real-time callbacks. Nothing is added to Tauri IPC.

Capture/reference blocks retain frame position, stream/epoch identity and optional monotonic
nanosecond timing estimates. A bounded consumer-side tracker aligns independent stream clocks
and estimates capture age plus predicted render lead. It does not measure acoustic latency or
change the existing occupancy-driven resampling. Cancellation, errors, epoch changes, expiry and
stream replacement invalidate retained observations. See the
[Phase 7 report](PHASE_7_RENDER_REFERENCE_IMPLEMENTATION_REPORT.md) for the exact contract,
buffering limits, consumer obligations and unverified hardware boundary.

### Opt-in AEC prototype (E16-04)

In the original native prototype, with `audio-engine/aec-prototype` and an explicit headless route association, the microphone
worker consumes timestamp-bearing primary capture and the incoming output's sole native reader.
It converts native render frames onto the microphone's 48 kHz grid, preserves stereo planes,
then runs worker-confined AEC3 before DFN. Fallback uses a separate measured 430-sample software
delay; DFN bypass still receives AEC output. Owner-only bounded handoff/return keeps route
lifecycles independent. No PCM or readers are added to Tauri IPC; the later desktop
integration below exposes only the preference and low-rate metadata.

ADR-014 and the original [prototype report](AEC_PROTOTYPE_IMPLEMENTATION_REPORT.md)
define the native-indexed history bound, original-age expiry, clock confidence, discontinuities,
FFI isolation, native allocations and the original broadband double-talk acceptance failure.

The subsequent [quality investigation](AEC_QUALITY_INVESTIGATION.md) makes AEC3 use the
route's explicit render-channel count from construction. Its internal automatic stereo
content detector is disabled so pauses cannot keep opposite-phase reference in a mono
downmix. This is worker-confined native configuration; callback, timestamp, route-ownership
and IPC contracts are unchanged.

The [quality fixes](AEC_QUALITY_FIXES.md) subsequently add a shared worker-owned AEC3 profile
for local-speech preservation and stereo delay recovery. A bounded, stateless native helper
keeps both speakers observable in the delay-estimation sidechain, falling back to the stronger
channel when averaging destroys excitation. The adaptive echo filters retain the original
stereo planes. The pinned native source receives a checked include/call-site patch; this is
a maintained native adjustment. The software quality gates pass.

The [desktop default integration](AEC_DESKTOP_DEFAULT_IMPLEMENTATION.md) enables microphone
AEC by default at the user's request, independently of denoise bypass. The microphone owner
acquires the incoming slot's sole render reader, including a later start or replacement;
the microphone output is never eligible. PCM stays native while availability and numeric
counters are exposed in snapshots. Invalid/missing references retain the aligned microphone
and DFN path with explicit unavailable status, so this fallback is not echo-protected. A saved
off preference applies after Stop/restart. Profile schema 3 migrates older missing preferences
to enabled without rewriting files on load. Physical speakerphone qualification remains open.
