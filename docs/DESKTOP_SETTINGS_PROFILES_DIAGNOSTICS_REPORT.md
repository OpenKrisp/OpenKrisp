# Desktop settings, profiles and diagnostics

## Scope and result

This change completes **E12-03, E12-05, E12-06 and E17-04**, starting from verified clean
`main` / `origin/main` at `f5673fa` (PR #10). It adds supported denoise editing for both routes,
saved-profile management, full route diagnostics and a sanitized report users can copy.
The remaining numbered backlog count decreases from 18 to **14**. No AEC or hardware acceptance
task is completed by this change.

## Behavior and ownership

`DenoiseControls` exposes the adapter's enabled flag, optional maximum attenuation in dB and
post-filter switch. Microphone mode and target latency are under its settings disclosure;
incoming retains its speech/bypass selector and also exposes a stopped-route latency setting.
The existing 40 ms default is retained; there are no new hardware-qualified latency presets.
Invalid latency, negative/non-finite attenuation and values overflowing Rust f32 are rejected
before Start or Apply. Device and latency changes require a stopped route.

Each route has an independent draft. Normal metric updates do not erase user edits, and an
authoritative mode change does not overwrite unrelated denoise edits. Reload or a new external
route restores its actual configuration. Native application sources expose their current denoise
settings but cannot be saved as an automatically restartable source profile.

Apply sends the existing bounded `update_route_settings` command. The UI waits for a matching
authoritative configuration snapshot, comparing attenuation at f32 precision. An IPC acceptance
alone is not displayed as a successful settings update. Rejection, route transitions and a
five-second confirmation timeout are visible. Confirmation means the engine accepted the
configuration; the existing protocol does not expose per-frame DSP acknowledgement. Stop remains
available while a settings update is pending.

No callback, worker, audio ring, route priority, native reference or model ownership code changes.
There is no new PCM transport, Tauri command, capability, clipboard plugin or generic file API.
All UI observations use the existing metadata snapshots. Model changes still execute on the
route's existing DSP worker.

## Profiles and migration

`Profiles` lists, refreshes, saves new profiles, updates/renames the selected profile, loads one
into its route controls, and deletes with an explicit confirmation. Each profile describes one
microphone or incoming route, not an automatically started pair. Saving uses the visible draft,
including settings not yet applied to active audio. No save or selection starts or stops a route.

Profile schema **2** adds explicit `sourceKind`, `destinationKind` and `processingMode` fields.
This prevents a virtual microphone ID from being restored as a physical output and preserves an
incoming profile's bypass intent. Incoming profiles require physical outputs; virtual endpoints
require explicit preferences. The existing atomic, serialized, bounded persistence worker is
retained. The command names and envelopes are unchanged; legacy requests without the added fields
retain the original physical-monitor semantics.

Schema 0/1 files migrate in memory with physical input/output roles and speech mode, preserving
their device preferences, latency and denoise settings. A successful explicit persistence
operation writes schema 2; opening the app alone never rewrites a profile file. Older app builds
cannot read schema 2 and will report their existing future-schema warning. Downgrade support is
not claimed. Corrupt/future files still fall back without being overwritten.

Loading a profile requires its target route to be stopped, checks again after the asynchronous
selection write, then fills that route's controls. A running microphone does not prevent loading
a stopped incoming route. Profiles selected on the previous launch appear selected in the panel,
but are not automatically applied. Explicit saved `default` preferences resolve to the current
physical default when loaded; missing named devices stay selected and fail normal Start
validation instead of silently switching to another device.

Failed saves preserve the UI draft and last confirmed collection. Concurrent profile operations
are guarded, and delayed initial reads cannot overwrite a newer refresh. An invalid-config
warning stays visible: loading fallback preferences does not persist anything; replacing the
unreadable configuration requires the explicitly labeled Save action. Deleting the final profile
retains the backend's valid default-profile behavior.

## Diagnostics and export

`DiagnosticPanel` shows both routes' negotiated rate/channel/sample/buffer formats, model identity
and frame/latency metadata, input/output levels, queue fill and extrema, inference last/p95/p99,
missed deadlines, overruns/underruns, discontinuities, resets and reset time, overload/skipped
inferences, drift, recovery results and native stream/callback counters. Connection loss labels
the snapshot as stale. Route latency is explicitly estimated and is not acoustic or measured
end-to-end latency. Phase 7's native-only timing tracker is not exposed through this UI.

Prepare report creates a frozen JSON snapshot; subsequent metric updates do not change the text
being copied. Refresh report explicitly captures a new one. The report is built from an allowlist
and capped at the engine's two route slots. It includes numeric formats/settings/counters and
known categorical statuses. Unknown categories become `other`, and non-finite numbers become
`null`. Device and route identifiers, device names, profile data, paths, free-form errors and
future unrecognized fields are excluded by construction. PCM is never present.

Copy report uses the standard WebView clipboard API when available. If unavailable or rejected,
the read-only report text is selected with instructions for ordinary manual Copy. No upload,
automatic logging, file write or clipboard access occurs without the user's report action.

## Validation (2026-09-10)

| Check | Result |
| --- | --- |
| `cargo ci-test --locked` | Passed: **183 tests**, 1 pre-existing ignored benchmark |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo ci-fmt` | Passed |
| `npm run --prefix apps/desktop test` | Passed: **46 tests** across 8 files |
| `npm run --prefix apps/desktop build` | Passed: TypeScript and Vite production assets |
| `npm run --prefix apps/desktop tauri -- build --no-bundle -- --locked` | Passed: optimized desktop executable with embedded frontend |
| `git diff --check` | Passed |

The 4 new Rust regressions cover schema-1 migration without implicit writes, role/mode persistence
across reopen, invalid role rejection before disk mutation, and final-profile deletion. Existing
Tauri invoke/persistence tests also pass against schema 2. The 20 new frontend tests cover draft
round trips/validation, both routes' live controls, snapshot confirmation, stale reads and edits,
save failures, load/start races, explicit deletion, missing devices, invalid-config fallback,
diagnostic privacy, report bounds and manual clipboard fallback.

Interaction tests mount the actual React components in jsdom with a mocked metadata API; they do
not claim to exercise native audio. `jsdom` 27.4.0 is pinned as a development-only dependency
compatible with the installed Node 24.12 runtime and CI's Node 24. The native production app was
also launched and its connected window visually inspected. No live audio-quality, physical
recovery, Bluetooth, conferencing, latency or long-duration result is inferred from that launch.

Local ignored evidence: `target/desktop-ui-ci-test.log`, `target/desktop-ui-clippy.log`,
`target/desktop-ui-production-build.log`, `target/desktop-ui-launch.log` and
`target/desktop-ui-production.png`. GitHub CI and PR status are separate from these local results.

## Remaining work

E10-05/E10-06 and E14-06 hardware validation remain open. Phase 6's two-hour concurrent runs,
device recovery, Bluetooth profile changes and conferencing acceptance are still unverified.
The earlier isolated missed deadline remains documented; this UI change does not resolve or
reclassify that observation. Platform-specific clipboard behavior beyond the tested manual
fallback and interactive Linux desktop acceptance also remain unverified.

Next implementation item: **E16-03**, evaluate AEC implementations and licenses, followed by a
bounded prototype before DeepFilterNet and dedicated echo tests. E17 still has seven open tasks,
including installer/model integrity, onboarding, restart hardening and hardware/support decisions.
