# Phases 0–4 Review Fixes

## Scope and outcome

This follow-up fixes reviewed defects in the existing physical microphone-monitor engine and
phase 4 control-plane bridge. It does not implement Phase 5, virtual routing, multi-route support,
or the remaining E12 desktop controls. Existing hardware acceptance remains open.

## Implementation and affected boundaries

| Area | Change | Main files |
|---|---|---|
| Output safety | Clamp and meter after output sinc resampling; preserve pre-resample non-finite protection | `audio-engine/src/worker.rs` |
| Model recovery | Restore pristine prepared model state; reapply current settings; coalesce stale audio accumulated during reset | `denoise-dfn/src/processor.rs`, `audio-engine/src/worker.rs` |
| Overload | 100-frame cooldown, one reset, then four contiguous healthy inference frames; failed reset never runs inference | `audio-engine/src/overload.rs`, `worker.rs` |
| Lifecycle | Separate cancellable route owner; observation-only snapshots; interruptible retry/activation/readiness waits | `audio-engine/src/engine/owned_route.rs`, `lifecycle.rs`, `headless.rs` |
| Observations | Snapshot publication precedes revisioned events; faults latch until route restart; retain one route snapshot | `audio-engine/src/engine.rs`, `audio-protocol/src/event.rs` |
| Desktop reconciliation | All events invalidate full state; single-flight/coalesced reads, one-second and focus repair, startup retry and listener cleanup | `apps/desktop/src/connection.ts`, `App.tsx`, `state.ts` |
| Persistence and quit | Bounded serialized disk worker, async mutation commands, snapshots independent of disk writes, drain accepted writes and join off the event loop | `apps/desktop/src-tauri/src/profile_writer.rs`, `config_store.rs`, `state.rs`, `lib.rs` |
| macOS packaging | Microphone usage description and audio-input entitlement wired into bundle configuration | `apps/desktop/src-tauri/Info.plist`, `Entitlements.plist`, `tauri.conf.json` |

Crate paths in this table are relative to `crates/` unless prefixed with `apps/`.

Public API changes are `EngineObservation` (flattened event payload plus `revision`),
`PipelineMetrics.resetLastUs/resetTotalUs` (default zero for older JSON), and explicit
`HeadlessMonitor.poll_health()`/`start_with_cancellation()`. `HeadlessMonitor.snapshot()` now takes
`&self` and never advances recovery. The CLI explicitly polls health and connects Ctrl-C to the
cancellation flag. Tauri command names, argument envelopes, event topics, and profile schema are
unchanged. Profile mutation commands now asynchronously await the persistence result.

The worker's former large free function is split into input and model/output stage ownership.
Callbacks still only convert/gate/transfer PCM through preallocated bounded rings and update
atomics. Neither snapshot locks nor disk queues are reachable from callbacks. The non-Send,
non-Sync DeepFilterNet model is still created, processed, reset and destroyed on its DSP thread.

## Regression evidence

Automated tests exercise the production worker graph, route-owner adapter, persistence
transaction/executor, Tauri invoke/event bridge, and frontend connection rather than relying only
on the standalone simulated route:

- Final output stays finite and at or below 0.98 at 44.1/48/96 kHz and ±1,000/0 ppm correction;
  output meters observe the post-resample clamped signal.
- Every retry budget from 0 through 255 terminates with exactly the configured attempt count.
- Three repeated non-silent reset cycles match fresh-model output within 1e-6, including runtime
  setting changes and warm-up; the reference output is checked to be non-zero.
- Audio injected while reset is deliberately blocked produces one coalesced recovery, not a reset storm.
- Recovery trials are contiguous and use one reset; reset failure stays dry until another cooldown.
- Stop/Shutdown during delayed startup cannot emit `Running` afterward; recovery progress remains
  readable while recovery waits; a failed, already-exited owner can still be stopped and joined.
- Snapshot revisions exist before their events; saturated event queues do not erase faults;
  2,000 different terminal route IDs leave only one retained snapshot.
- Frontend tests cover initially absent routes, reordered/stale events, burst coalescing, dropped
  events, timer/focus repair, startup retry, and late subscription/in-flight cleanup on unmount.
- Slow and failed persistence preserves readable committed state and existing disk data;
  concurrent mutations do not lose updates; a full writer queue fails explicitly; accepted writes
  drain in order even if a caller abandons its reply.

### Production-model synthetic recovery smoke

On the local Apple Silicon macOS host, the opt-in release test runs the actual DeepFilterNet
adapter and production worker with synthetic 48 kHz callbacks for four seconds. It injects one
input overflow after one second and verifies that continuous output resumes without repeated
resets. No microphone, speaker, or other physical endpoint is opened.

Observed run: one reset, `resetLastUs=179`, `resetTotalUs=179`, zero output underruns,
`inferenceP99Us=766`, and `degraded=false`. The pre-fix review probe measured roughly 72–74 ms
per full reconstruction and 41 resets/297 underruns in three seconds after the injected overflow.
These are local measurements, not portable timing guarantees or a substitute for the long soak.

The timing test is intentionally ignored by the default debug/CI suite because it depends on
release optimization and host scheduling; it was run explicitly here:

```text
cargo test --locked --offline --release -p audio-engine --test dfn_recovery -- --ignored --nocapture
```

## Verification commands

All gates below passed locally on macOS: 130 Rust tests and 12 frontend tests. The default Rust
suite leaves the wall-clock recovery smoke intentionally ignored; that smoke passed separately
in release mode. No dependencies or lockfiles changed.

```text
cargo ci-fmt
cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings
cargo ci-test --locked --offline
cargo deny --offline check licenses
cargo run --locked --offline --release -p denoise-dfn --example offline_inference
npm run --prefix apps/desktop typecheck
npm test --prefix apps/desktop
npm run --prefix apps/desktop build
npm run --prefix apps/desktop tauri -- build --debug --bundles app -- --locked --offline
```

The generated debug `.app` was inspected with `plutil`: its `Contents/Info.plist` contains the
microphone description, and both source plists validate. The checked-in entitlement is wired to
Tauri's signing configuration. This debug build has only the linker's ad-hoc signature, without
sealed resources or embedded application entitlements; it is not a Developer ID signed release.

## Limits and next acceptance work

- Cancellation interrupts application-controlled waits in slices of at most 5 ms. A native CPAL
  call, model construction/inference already executing, or filesystem sync cannot be forcibly
  interrupted. Cleanup waits for that operation on its owner; it never detaches native/model
  resources. The supervisor/UI remains responsive during route startup/recovery, but these changes
  do not promise a hard wall-clock shutdown deadline for a hung OS/backend.
- Pristine model restoration still allocates/copies state, and upstream inference still allocates.
  Neither operation belongs in a callback. Inference deadlines exclude reset work, which is now
  visible through separate timing counters.
- Clearing all input accumulated during discontinuity recovery deliberately drops stale audio.
  Output underruns remain observable and do not themselves reset the model timeline.
- Explicit route restart acknowledges latched route faults. This single-route phase retains the
  current/latest terminal route, not an ever-growing route history.
- No fresh physical microphone permission prompt, denial/retry, Bluetooth disconnect/reconnect,
  sleep/resume, audible shutdown, two-hour soak, signed distribution, or Windows/Linux runtime
  acceptance is claimed by this follow-up. Cross-platform compilation remains the CI gate.

This hardens already-completed E2/E3/E7/E8/E9/E11 behavior; it does not claim any new epic or phase
completion. The recommended next work within the user's boundary is the existing Phase 2/3/4
physical acceptance checklist in `PHASE_2_HARDWARE_TEST.md`, followed by a separately authorized
remaining E12 UI slice. Do not start Phase 5 as part of these fixes.
