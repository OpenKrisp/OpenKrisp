# Autonomous Coding Assistant Guide

## 1. Mission

Implement the Rust/Tauri real-time speech-noise-reduction application described in this documentation pack. Work incrementally, preserve the real-time invariants, and produce a tested repository after every phase.

## 2. Documents to read before coding

Read in this order:

1. Root `README.md`.
2. `docs/01_PRODUCT_SCOPE.md`.
3. `docs/02_SYSTEM_ARCHITECTURE.md`.
4. The subsystem document relevant to the assigned task.
5. `docs/09_TEST_STRATEGY.md`.
6. `docs/11_DECISIONS_RISKS_AND_OPEN_QUESTIONS.md`.
7. `docs/12_IMPLEMENTATION_BACKLOG.md`.

For a new repository, begin with Phase 0 in `docs/08_IMPLEMENTATION_ROADMAP.md`.

## 3. Non-negotiable rules

- Do not send PCM through Tauri commands, events, channels, JSON, JavaScript arrays, or Web Audio.
- Do not run DeepFilterNet in a CPAL callback.
- Do not block, allocate intentionally, perform I/O, or emit regular logs from CPAL callbacks.
- Do not use unbounded queues for audio or engine commands.
- Do not assume input and output clocks are synchronized.
- Do not hard-code DeepFilterNet frame sizes until the pinned upstream revision has been inspected.
- Do not assume the DeepFilterNet processor is `Send` or `Sync`; construct it inside its DSP worker.
- Do not begin custom virtual-driver development during the core MVP.
- Do not hide stream failures or substitute silent failure paths without metrics.
- Do not use `unwrap` or `expect` in production route lifecycle code.
- Do not change public architecture decisions silently. Record deviations in an ADR or architecture-verification document.

## 4. Required workflow for every assignment

### Step 1: Inspect

- Read the relevant documents and repository code.
- Identify existing conventions and constraints.
- Verify current upstream APIs from the repository lockfile, pinned revision, or official source.
- List assumptions that materially affect implementation.

### Step 2: Plan

Create a concise implementation plan containing:

- Files to add or change.
- Public APIs affected.
- Threading and ownership implications.
- Real-time safety considerations.
- Tests to add.
- Known risks.

Do not broaden the task into unrelated refactors.

### Step 3: Implement

- Keep each component within its documented crate boundary.
- Allocate buffers during initialization.
- Use explicit units in type and variable names.
- Return typed errors.
- Keep route cleanup deterministic.
- Add comments only where they explain a non-obvious invariant or safety requirement.

### Step 4: Test

At minimum:

- Run formatting.
- Run Clippy with warnings denied.
- Run affected unit and integration tests.
- Add deterministic tests for new timing or buffering behavior.
- Report platform/hardware tests that could not be run.

### Step 5: Report

Return:

1. Summary of implementation.
2. Files changed.
3. Important design choices.
4. Tests run and results.
5. Remaining risks or unverified hardware behavior.
6. Backlog items completed.
7. Recommended next backlog item.

## 5. Architecture verification task

Before implementing the full engine, verify these assumptions against the pinned dependencies:

- Exact DeepFilterNet Rust crate/import path.
- Model loading API.
- Model input layout.
- Model sample rate.
- Model frame/hop size.
- Reported or derived algorithmic latency.
- Available runtime parameters.
- Whether processor state is `Send` or `Sync`.
- CPAL host/backend behavior on the target platform.
- CPAL supported sample types and stream error semantics.
- Whether CPAL exposes the required loopback source on the target platform.

Write findings to `docs/ARCHITECTURE_VERIFICATION.md`. Update code types to reflect verified facts, but do not weaken the core real-time boundaries.

## 6. Preferred implementation style

- Favor small explicit components over one large `AudioEngine` file.
- Favor composition over generic abstraction layers that are not yet needed.
- Introduce traits where they enable deterministic testing or backend isolation.
- Keep callback closures small and reviewable.
- Use newtypes for identifiers and explicit units.
- Use bounded channels with clear full/disconnected behavior.
- Keep model-specific arrays inside `denoise-dfn`.
- Keep Tauri-specific DTO conversion inside the desktop application.

## 7. Real-time review checklist

For every code path reachable from a CPAL callback, confirm:

- No lock acquisition that can block.
- No channel send that can block.
- No heap allocation in steady state.
- No formatting or logging.
- No panic on queue full/empty.
- Queue operations have a constant bounded cost.
- Error handling writes valid output and returns quickly.

For the DSP worker, confirm:

- Buffers are reused.
- Work is bounded per model frame.
- Queue overflow cannot grow latency without limit.
- Cancellation and shutdown are checked.
- Reconfiguration occurs between frames.
- Model errors enter aligned fallback.

## 8. Testing expectations

Never replace deterministic tests with manual listening alone.

When adding a component:

- Pure DSP: unit tests with known arrays.
- Queue behavior: randomized chunk/property-style tests.
- Clock behavior: deterministic simulated clocks.
- Model adapter: fixture and error tests.
- CPAL integration: a manual/hardware test protocol plus automated logic tests.
- Tauri bridge: command/DTO/state tests without PCM.

## 9. Scope control

Stop and report rather than inventing behavior when:

- The pinned DeepFilterNet API contradicts a required type.
- A platform feature requires a signed driver or privileged installer not present in scope.
- A license prevents bundling a dependency or metric.
- The selected resampler cannot support bounded worker processing.
- Hardware-specific behavior cannot be verified in the current environment.

Provide the smallest safe alternative and record the unresolved item.

## 10. Initial prompt for the coding assistant

Use this prompt together with the repository and documentation pack:

```text
You are implementing a real-time audio noise-reduction desktop application in Rust for macOS and Linux.

Read README.md and every document referenced by the current roadmap phase. Begin with Phase 0 unless the repository clearly shows that it is complete.

The stack is Tauri v2 for the control/UI layer, CPAL for audio-device streams, and DeepFilterNet for speech noise reduction. The PCM data path must remain entirely in Rust. CPAL callbacks may only convert samples and transfer them through preallocated bounded buffers. They must never run inference, block, perform I/O, or send PCM to Tauri. Construct and use each DeepFilterNet processor inside its dedicated DSP worker thread.

Before coding, inspect the repository and produce a focused plan. Verify the exact APIs and model metadata of the pinned dependencies rather than relying on assumptions. Implement only the assigned roadmap phase or backlog epic, including tests. Preserve bounded latency and deterministic cleanup. Do not begin virtual-driver work before the physical microphone-monitor engine is stable.

At completion, run formatting, Clippy, and applicable tests. Report files changed, design decisions, test results, limitations, architecture deviations, completed backlog IDs, and the next recommended task.
```

## 11. Suggested first assignment

```text
Implement Phase 0 from docs/08_IMPLEMENTATION_ROADMAP.md.

Create the Rust workspace skeleton, CI commands, a CPAL device-enumeration example, and a minimal offline DeepFilterNet inference spike. Pin all relevant revisions. Write docs/ARCHITECTURE_VERIFICATION.md with the verified model sample rate, frame size, input layout, latency, settings, and thread-safety properties. Do not implement the live audio pipeline yet.
```
