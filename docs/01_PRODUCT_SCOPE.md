# Product Scope and Requirements

## 1. Product statement

Build a desktop application that reduces background noise from live microphone audio and, where supported, speech-oriented speaker audio. The application must provide a simple Tauri interface while keeping the complete real-time audio path in Rust.

The first usable release should let a user select a physical microphone and a headphone or speaker output, enable DeepFilterNet, and hear the cleaned microphone with stable bounded latency. Later releases can expose the cleaned signal as a virtual microphone and process audio captured from other applications.

## 2. Primary goals

- Capture live microphone audio with CPAL.
- Run DeepFilterNet continuously with no unbounded buffering.
- Play cleaned audio through a selected CPAL output device.
- Provide device selection, start/stop, bypass, processing-strength controls, meters, and diagnostics through Tauri.
- Recover gracefully from device changes, stream errors, sleep/resume, and temporary processing overload.
- Target macOS and Linux through a shared core with isolated platform-routing adapters.
- Make the audio engine independently testable without Tauri or physical devices.

## 3. Secondary goals

- Expose cleaned microphone audio to conferencing and recording applications through a virtual microphone.
- Capture system or application audio and clean speech before playback.
- Run microphone and speaker processing simultaneously.
- Preserve user profiles and preferred devices.
- Provide actionable latency, underrun, overrun, and model-performance diagnostics.

## 4. Non-goals for the first release

- Windows support has been removed; see [the removal decision](WINDOWS_SUPPORT.md).
- Building and signing custom virtual audio drivers for all operating systems.
- General-purpose restoration of music, games, films, or arbitrary sound effects.
- Acoustic echo cancellation in the first microphone-monitor MVP.
- Cloud inference or transmission of PCM audio.
- Recording, transcription, voice conversion, or voice cloning.
- A browser-based audio engine.
- Perfectly identical behavior across every audio backend and Bluetooth device.

## 5. Supported operating modes

### 5.1 Clean microphone monitor

```text
Physical microphone -> CPAL input -> DeepFilterNet -> CPAL output -> headphones
```

Purpose: validate the engine and allow the user to hear the cleaned signal locally.

### 5.2 Virtual microphone

```text
Physical microphone -> DeepFilterNet -> virtual microphone endpoint -> Zoom/Discord/OBS/etc.
```

Purpose: let other applications consume the cleaned microphone.

### 5.3 Clean incoming speech

```text
Application PCM or system loopback -> DeepFilterNet -> physical speakers/headphones
```

Purpose: reduce noise in calls, podcasts, or speech recordings. Processing must be opt-in because DeepFilterNet is speech-oriented.

### 5.4 Full duplex

```text
Microphone route -> virtual microphone
Incoming speech route -> physical output
```

Purpose: process both sides of a call. Open-speaker use will eventually require a separate acoustic echo cancellation stage.

## 6. Functional requirements

### Device management

- **FR-001:** Enumerate input and output devices.
- **FR-002:** Show stable internal identifiers plus user-facing device names.
- **FR-003:** Show the current default input and output devices.
- **FR-004:** Refresh device lists after hotplug or explicit user action.
- **FR-005:** Persist preferred devices while allowing fallback to the current default.
- **FR-006:** Report the negotiated sample format, sample rate, channel count, and buffer size.

### Route lifecycle

- **FR-010:** Start a microphone-monitor route.
- **FR-011:** Stop a route without leaving streams or worker threads alive.
- **FR-012:** Prevent duplicate starts for the same route identifier.
- **FR-013:** Expose route states: `Stopped`, `Starting`, `Running`, `Recovering`, `Failed`, and `Stopping`.
- **FR-014:** Reconfigure safe model parameters without rebuilding the CPAL streams.
- **FR-015:** Rebuild a route when device or format changes require it.

### Noise reduction

- **FR-020:** Load a pinned DeepFilterNet model from an application-controlled asset.
- **FR-021:** Validate the model asset before starting inference.
- **FR-022:** Process complete model frames at the model-required sample rate.
- **FR-023:** Reset model state after major audio discontinuities.
- **FR-024:** Provide a latency-aligned bypass path with a short crossfade.
- **FR-025:** Apply a safety limiter or clamp after model output.
- **FR-026:** Preserve stream duration and channel ordering.

### Monitoring and diagnostics

- **FR-030:** Publish input and output peak/RMS meters at a low fixed rate.
- **FR-031:** Publish input-overrun and output-underrun counters.
- **FR-032:** Publish model inference duration statistics, including at least p95 and p99.
- **FR-033:** Publish current queue fill levels and estimated route latency.
- **FR-034:** Report recoverable and fatal errors with machine-readable codes.
- **FR-035:** Make diagnostic logs available without logging from the audio callbacks.

### Persistence

- **FR-040:** Save user preferences outside the real-time path.
- **FR-041:** Version the persisted configuration schema.
- **FR-042:** Ignore or migrate incompatible configuration safely.

## 7. Non-functional requirements

### Real-time safety

- **NFR-001:** CPAL callbacks must not wait on the model or UI.
- **NFR-002:** No intentionally unbounded queue may carry audio or control messages.
- **NFR-003:** Steady-state callback execution should perform no heap allocation.
- **NFR-004:** Model processing must occur outside CPAL callbacks.
- **NFR-005:** Route latency must stay within a configured bound under recoverable overload.

### Performance

- **NFR-010:** At the target hardware tier, p99 model processing time should remain comfortably below the model-frame deadline.
- **NFR-011:** The engine must run for multiple hours without queue growth, thread leaks, or memory growth attributable to the audio loop.
- **NFR-012:** Metrics and UI updates must not cause audible glitches.
- **NFR-013:** Start and stop operations must be deterministic and idempotent.

### Reliability

- **NFR-020:** A device disconnect must transition the route to recovery or failure rather than panic the process.
- **NFR-021:** An output underrun must write silence or a controlled fallback and continue.
- **NFR-022:** An input overflow must keep latency bounded by discarding stale data according to a documented policy.
- **NFR-023:** A worker panic must be observable by the supervisor.
- **NFR-024:** Shutdown must join all owned threads where possible.

### Security and privacy

- **NFR-030:** Raw PCM stays local unless a future feature explicitly and transparently exports it.
- **NFR-031:** Model assets must be loaded from trusted application paths and checked against a manifest or hash.
- **NFR-032:** Tauri permissions and commands must follow least privilege.
- **NFR-033:** User-controlled paths must not be passed unchecked to native loaders.

## 8. Initial success criteria

The microphone-monitor MVP is successful when all of the following are true:

- A user can select a microphone and output device and start processing.
- Speech is audibly cleaner on representative noisy recordings and live tests.
- The application can run continuously for at least two hours without a crash, deadlock, or steadily increasing latency.
- Device callbacks remain responsive even when the model temporarily misses a deadline.
- Bypass transitions do not create a large click or change timing abruptly.
- The UI displays live meters, route state, negotiated formats, queue health, and underrun/overrun counts.
- Automated tests cover callback-size variation, model-frame assembly, sample-rate conversion, and lifecycle cleanup.

## 9. Release boundaries

### Release A: Engine proof of concept

- CLI or headless test harness.
- One physical input and one physical output.
- DeepFilterNet processing.
- Basic metrics.

### Release B: Desktop MVP

- Tauri UI.
- Device selection and profiles.
- Start, stop, bypass, and diagnostics.
- Robust recovery for common device errors.

### Release C: Virtual microphone integration

- Platform routing adapter.
- Existing virtual-audio endpoint integration or a separately installed endpoint.
- Loop-prevention validation.

### Release D: Speaker speech processing

- Application-owned PCM first.
- Platform loopback capture second.
- Explicit speech-only mode.

### Release E: Full-duplex production hardening

- Concurrent routes.
- Render-reference plumbing for future AEC.
- Expanded platform tests, installer integration, and optional background service.
