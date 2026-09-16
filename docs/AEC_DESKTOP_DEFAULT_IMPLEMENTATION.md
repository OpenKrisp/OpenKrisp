# Desktop AEC enabled by default

The user explicitly requested default desktop enablement after the software quality
fixes. This supersedes the earlier decision to withhold the desktop feature until
physical acceptance. **It does not establish speakerphone quality or complete
E16-05/E16-06.** The physical, conferencing and sustained-use gates remain open.

## Behavior

- Desktop microphone starts enable WebRTC AEC3 before DeepFilterNet by default.
  Echo cancellation remains enabled when microphone noise reduction is bypassed or
  disabled. Incoming playback is never echo cancelled.
- The microphone panel has a checked **Echo cancellation** option. Stop the microphone
  to change it, then restart. An explicit off preference survives saved profiles.
  The app does not automatically start audio routes or change system devices.
- AEC automatically follows the engine's sole incoming playback slot. Either route
  may start first. There is no hard-coded desktop route ID, and the microphone's
  output slot can never supply its own reference.
- Status distinguishes active cancellation, waiting for incoming audio, alignment,
  unstable timing, overload, native failure, stopped and disconnected states. An
  active label requires both routes to be running and the worker to report Running.
  It is processing status, not a measured echo-reduction or quality guarantee.
- Without a usable reference, the microphone continues through the existing aligned
  fallback and DFN path. **That fallback is not echo-protected.** The UI says so;
  it never treats a checked preference as proof of active cancellation.

The call's speaker output must enter OpenKrisp's incoming route. Other applications'
unreferenced playback is outside cancellation coverage. macOS currently exposes one
BlackHole 2ch bus: using that same bus for incoming playback and outgoing virtual
microphone remains rejected. Default enablement does not solve the separate-bus
desktop conferencing requirement or install another driver. Linux's existing
separate incoming/outgoing PipeWire endpoints remain supported by routing policy;
this change does not claim a new Linux hardware test.

## Native and control-plane boundaries

The desktop explicitly compiles `audio-engine/aec-prototype`. Its production driver
creates a fresh AEC configuration/diagnostics object only for an enabled microphone
route. Builds of the engine without that feature reject enabled microphone requests
instead of silently ignoring them. Standalone headless monitors still opt in explicitly.

An optional native reference route ID now permits either a pinned test association or
automatic selection of the incoming slot. Acquisition and return stay on route owner
threads, using the existing registry and capacity-one worker handoff. Replacement
closes taken readers. Returned readers are accepted only if their native stream
identity still matches the slot and they remain open; stale generations cannot
replace a new one. Stop/recovery destroys learned state and waits for fresh alignment.
No route owner joins the other route or shares its DeepFilterNet state.

All capture/render PCM remains native. Only the enable preference, status enum and
numeric counters were added to protocol snapshots/Tauri IPC. The sanitized diagnostic
report allowlists those fields. Callbacks, clock domains and timestamp units,
render placement, 200 ms expiry, 32,768-frame reference history, clock fitting,
AEC/DFN order and worker ownership are unchanged. AEC still contributes a 430-sample
software impulse-peak delay to the aligned fallback and estimated route latency;
this is not measured device/acoustic or end-to-end latency.

Profile schema 3 adds `echoCancellation`. Missing fields in older microphone profiles
default to true, while explicit false remains false. Schemas 0/1/2 migrate in memory;
loading alone does not rewrite the user's file. A later explicit save persists schema 3.
Incoming requests normalize the preference to false. PCM or reference buffers remain
rejected at the command boundary.

The app bundle now includes the pinned native notices, provenance README and archive
hash manifest under `third-party/aec/`. E17-07's broader notice review stays open.

## Validation

Regression coverage includes the real Tauri command boundary, missing-field defaults,
explicit opt-out, profile migration/reopen, native reference handoff to actual AEC,
incoming Stop/replacement and microphone restart ownership, and UI status changes
when either route stops or recovers. The existing quality, stereo, clock, callback,
allocation and AEC-before-DFN/bypass tests remain enforced.

Final macOS checks passed:

- `cargo ci-test --locked`: **216 passed, two opt-in timing benchmarks ignored**.
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`.
- `cargo ci-fmt` and `git diff --check`.
- `npm run --prefix apps/desktop test`: **49 passed**.
- Production frontend build (`tsc --noEmit` and Vite), run by Tauri's build hook.
- `python3 -m unittest discover -s scripts/tests -v`: **11 passed**.
- Dedicated native quality check: **15 passed, none ignored**, plus seven compiled
  alignment contracts. The earlier native profile is unchanged.
- `cargo check --locked -p audio-engine --no-default-features --lib`.
- `npm run --prefix apps/desktop tauri -- build --bundles app`: release app built at
  `target/release/bundle/macos/OpenKrisp.app`. All ten native notice/author files,
  provenance README and SHA-256 manifest match their packaged copies byte for byte.

[The evidence manifest](evidence/aec-desktop-default-2026-09-15.json) retains source,
log and bundle hashes. No new remote CI or Linux desktop result is claimed. The new
app is built locally; an already running or installed older binary is not replaced.

No microphone recording, real-room listening, actual conference, Bluetooth transition,
device recovery or two-hour hardware run was performed for this change. Earlier
physical tests remain historical and do not qualify this default-enabled desktop build.
