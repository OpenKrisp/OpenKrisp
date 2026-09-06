# Phase 4 Control-Plane Implementation Report

## Scope

This change completes backlog items E8-07 through E8-09 and epic E11. It implements the roadmap's
recommended Tauri command/event bridge before the full desktop UI: an application-owned engine
service, stable snapshot DTOs, bounded commands and events, validated Tauri commands, profile
persistence, tray lifecycle, and a connected React shell. It does not claim the device selectors,
route controls, meters, diagnostics, profiles UI, or permission onboarding in E12.

## Engine service and IPC boundary

`AudioEngine` owns a named supervisor thread, one production CPAL-backed route, a bounded command
receiver, a bounded low-rate event sender, and deterministic shutdown. Its cloneable handle exposes
only non-blocking command submission, immutable snapshots, and an out-of-band shutdown signal. The
signal remains usable when the normal command queue is saturated and causes the supervisor to stop
and join every live route resource.

`EngineSnapshotStore` publishes revisioned point-in-time state under a control-plane lock that no
audio callback can reach. Route snapshots contain configuration, negotiated formats, device
descriptors, model metadata, counters, meters, latency, and sanitized faults. They contain no PCM,
backend handles, model objects, or callback buffers.

The engine emits four low-rate observations through a bounded channel:

- `audio://state-changed` for lifecycle transitions;
- `audio://devices-changed` as an inventory invalidation signal;
- `audio://metrics` at no more than 10 Hz;
- `audio://fault` for sanitized recoverable and terminal errors.

Event delivery never blocks the supervisor. If the desktop consumer is slow, redundant events can
be dropped and the revisioned snapshot remains authoritative.

## Desktop commands and validation

The Tauri bridge exposes only explicit device, snapshot, route, bypass, settings, and profile
commands. Route requests are strict camel-case DTOs with unknown fields denied. The trusted Rust
boundary validates identifiers, physical input/output endpoint roles, non-zero latency capped at
1,000 ms, and finite supported denoise settings before enqueueing. The engine validates the domain
configuration again before touching route resources.

Long operations do not run on Tauri command handlers. A successful mutating route command means
that the bounded engine queue accepted it; subsequent state or failure is observed through an event
or snapshot. Internal CPAL, model, and worker errors are mapped to stable error codes and do not
leak local paths or backend diagnostics to the WebView.

The React state layer installs event listeners before taking its initial snapshot. It treats device
events as invalidations, rejects stale asynchronous snapshots by revision, and uses a full snapshot
after startup or reconnect rather than assuming that its event history is complete.

## Profiles, lifecycle, and security

Profiles use a versioned schema with strict validation. Schema-zero input is migrated in memory;
invalid, corrupt, or future-version input falls back safely without overwriting the original file.
Explicit mutations are serialized, written to a temporary file, synced, and atomically persisted
before the in-memory view changes.

Closing the main window hides it while the tray keeps the engine alive. The tray can restore the
window or explicitly quit. Quit requests engine shutdown, joins the supervisor, releases streams,
and then joins the event bridge. Startup failures return a non-zero process exit status.

The bundled main window has no shell, filesystem, network plugin, arbitrary path, or generic command
permission. Its Tauri capability permits only the eleven named OpenKrisp commands and core event
listen/unlisten operations. Production content is local, its CSP excludes development endpoints,
and model resolution stays in trusted Rust code against the packaged resource directory.

Tauri's transitive graph adds explicitly declared BSD-3-Clause, ISC, MPL-2.0, and Apache-2.0 with
LLVM-exception components. The dependency gate allows those exact SPDX expressions. MPL is
file-level copyleft rather than permissive, so final third-party notices and redistribution
compliance remain part of the existing release legal review.

## Automated evidence

Focused tests cover:

- immutable snapshot revisions and JSON round trips without PCM;
- command queue saturation, validation, lifecycle updates, failed-route stop, and shutdown joins;
- strict desktop request conversion and stable serialized error shape;
- profile persistence, migration, corrupt/future fallback, and warning replacement;
- stable event-topic mapping and bounded tray icon construction;
- frontend snapshot reconciliation, stale-revision rejection, and error normalization;
- strict TypeScript checking and a production Vite build.

The repository formatting, warnings-denied Clippy, locked workspace tests, dependency-license gate,
offline model spike, and desktop runtime launch smoke are the local acceptance gates. Pull-request
CI remains authoritative for Linux, macOS, and Windows compilation.

## Desktop runtime smoke

On 2026-09-06, the normal embedded-asset binary launched on the primary macOS host, rendered the
connected shell, and reconciled a CoreAudio inventory of four devices with a stopped route and the
default profile. Closing the window left the process running, the tray's **Show OpenKrisp** action
restored it, and the tray's **Quit** action shut it down cleanly. This smoke enumerated devices but
did not start a route or open a microphone stream.

A debug macOS `.app` bundle also built and launched successfully with its Vite assets, model
archive, adjacent manifest, and icon present under the bundle resources. This caught and fixed the
missing default `custom-protocol` feature that had initially made a plain Cargo launch target the
inactive development server instead of embedded assets.

## Remaining Phase 4 work

- Implement E12's device selectors and start/stop state flow.
- Add bypass and verified denoise controls.
- Render meters and the detailed diagnostic snapshot.
- Add profile management and permission/unsupported-feature messaging.
- Exercise the complete interactive UI against physical devices; the desktop launch smoke does not
  open a microphone route by itself.
