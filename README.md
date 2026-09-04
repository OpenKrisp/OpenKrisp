# Rust Audio Noise Reduction Desktop App

This documentation pack describes a cross-platform desktop application that performs real-time speech noise reduction for microphones and speaker audio.

The reference stack is:

- **Tauri v2** for the desktop shell, settings UI, tray integration, and control plane.
- **Rust** for all audio, DSP, model inference, routing, and lifecycle management.
- **CPAL** for physical audio-device discovery and real-time input/output streams.
- **DeepFilterNet** for speech-oriented real-time noise reduction.
- **Bounded lock-free or wait-free queues** between real-time callbacks and processing workers.

The documents are intentionally divided by subsystem so they can be supplied to an autonomous coding assistant one at a time.

## Recommended reading order

1. [Product Scope and Requirements](docs/01_PRODUCT_SCOPE.md)
2. [System Architecture](docs/02_SYSTEM_ARCHITECTURE.md)
3. [Real-Time Audio Engine](docs/03_REALTIME_AUDIO_ENGINE.md)
4. [DeepFilterNet Integration](docs/04_DEEPFILTERNET_INTEGRATION.md)
5. [Tauri Application and IPC](docs/05_TAURI_APP_AND_IPC.md)
6. [Platform Routing](docs/06_PLATFORM_ROUTING.md)
7. [Rust Workspace and Public APIs](docs/07_RUST_WORKSPACE_AND_APIS.md)
8. [Implementation Roadmap](docs/08_IMPLEMENTATION_ROADMAP.md)
9. [Test Strategy](docs/09_TEST_STRATEGY.md)
10. [Autonomous Coding Assistant Guide](docs/10_AUTONOMOUS_AGENT_GUIDE.md)
11. [Architecture Decisions, Risks, and Open Questions](docs/11_DECISIONS_RISKS_AND_OPEN_QUESTIONS.md)
12. [Implementation Backlog](docs/12_IMPLEMENTATION_BACKLOG.md)

## Core architectural invariants

These rules apply to every implementation phase:

1. **PCM audio never crosses Tauri IPC.** The WebView receives only configuration, status, errors, and low-rate metrics.
2. **CPAL callbacks never run model inference.** They only convert and transfer samples through preallocated bounded buffers.
3. **CPAL callbacks never block, allocate intentionally, wait on a mutex, perform file I/O, or emit normal logs.**
4. **Each active processing route owns its own DeepFilterNet state.** Microphone and speaker routes do not share one recurrent model instance.
5. **The DeepFilterNet object is created and used on the same DSP worker thread.** This avoids relying on the model runtime being `Send` or `Sync`.
6. **48 kHz is the canonical internal model rate.** Device-native rates are converted outside the model adapter.
7. **Input and output devices are separate clock domains.** The design must tolerate long-term drift even when both report the same nominal sample rate.
8. **Latency remains bounded.** On overload, discard stale audio or enter a controlled bypass rather than allowing queues to grow indefinitely.
9. **System-wide routing is a platform concern.** CPAL handles streams; virtual microphones, loopback sources, and virtual speakers require platform-specific routing support.
10. **DeepFilterNet is used for speech-oriented enhancement.** Non-speech content should bypass processing by default.

## Suggested handoff to an autonomous coding assistant

Give the assistant:

1. This `README.md`.
2. `docs/10_AUTONOMOUS_AGENT_GUIDE.md`.
3. The document for the current implementation phase.
4. Any existing repository files and constraints.

Ask it to implement only one roadmap phase or backlog epic at a time. Require tests and an implementation report before moving to the next phase.

## Reference implementation assumptions

The proposed first implementation assumes:

- Tauri v2.
- Vite and TypeScript for the frontend; React is an acceptable default but is not architecturally required.
- A Rust workspace with independent crates for protocol types, audio I/O, DSP, DeepFilterNet, engine orchestration, and platform routing.
- A local microphone-to-headphones MVP before attempting virtual audio devices.
- A low-latency DeepFilterNet model pinned to a known upstream revision and validated during dependency setup.

Exact upstream APIs, model frame sizes, and crate features must be verified against the pinned dependency revision during implementation.

## Implementation status

Roadmap Phases 0 and 1 and the Phase 2 headless software path are implemented. In addition to the
hardware-independent foundation, the repository now includes a hash-validated production
DeepFilterNet adapter, CPAL inventory and typed callbacks, a thread-confined pipeline worker, a
single-route supervisor, lock-free diagnostics, and the `openkrisp-monitor` CLI.

Local microphone capture, processing, output callbacks, underrun recovery, and clean shutdown have
passed a 10-second physical smoke test. Phase 2 hardware acceptance is still pending: cleaned
output must be evaluated by a human listener and the two-hour baseline must be recorded on the
supported device matrix. The exact safety steps, metrics, and pass criteria are in
[`docs/PHASE_2_HARDWARE_TEST.md`](docs/PHASE_2_HARDWARE_TEST.md). Implementation evidence and known
limitations are in
[`docs/PHASE_2_IMPLEMENTATION_REPORT.md`](docs/PHASE_2_IMPLEMENTATION_REPORT.md).

Verified upstream details and remaining constraints are recorded in
[`docs/ARCHITECTURE_VERIFICATION.md`](docs/ARCHITECTURE_VERIFICATION.md). Phase 1 design choices,
measurements, and limitations are recorded in
[`docs/PHASE_1_IMPLEMENTATION_REPORT.md`](docs/PHASE_1_IMPLEMENTATION_REPORT.md).

Useful local commands:

```text
cargo ci-fmt
cargo ci-clippy
cargo ci-test --locked
cargo deny check licenses
cargo run --locked -p audio-cpal --example list_devices
cargo run --locked -p denoise-dfn --example offline_inference
cargo run --release --locked -p audio-engine --bin openkrisp-monitor -- --list-devices
cargo run --release --locked -p audio-engine --bin openkrisp-monitor -- --input "INPUT_ID" --output "OUTPUT_ID"
npm --prefix apps/desktop run typecheck
```
