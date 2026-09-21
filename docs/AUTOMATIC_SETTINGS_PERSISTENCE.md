# Automatic desktop settings persistence

## Scope

Implemented on `codex/automatic-settings`, based on verified `main` at `29bc1f6` (PR #14).
GitHub SSH timed out; the authenticated GitHub API confirmed the main commit before branching.

The desktop now remembers microphone and incoming preferences automatically. Manual profile
management, its four IPC commands, profile storage and all migration code have been removed.
At the user's request there is no import or compatibility layer. Old `profiles.json` files are
unused and untouched. A new `settings.json` starts with desktop defaults and eligible devices.
This supersedes the original E11-07/E12-06 profile UX, without closing any other numbered task.

## User behavior

Both routes restore device identifiers, destination role, processing mode, target latency in
milliseconds, noise reduction settings and the microphone AEC preference. AEC defaults on for
the microphone; incoming defaults to aligned bypass and has no capture AEC. Both retain 40 ms
target latency and the desktop's previous post-filter-off default. Choosing AEC off persists.

Controls wait for the saved settings and initial device inventory before editing or Start.
For empty preferences, the UI selects the unique dedicated endpoint from each router role:
OpenKrisp Microphone for outgoing audio and OpenKrisp Speaker for incoming. It selects eligible
physical system defaults, excluding virtual/aggregate/unknown devices, moving PipeWire aliases,
and endpoint ancestry. Ambiguous endpoint discovery remains unselected. Missing installed
macOS drivers remain selected with the existing setup instructions and disabled Start.
A missing named physical device stays selected; the user can reconnect it or choose another.
Inventory refresh never substitutes a different physical device. These choices resolve once
at launch, not continuously as system defaults change.

Initial resolved choices and valid user edits save automatically. An incomplete route may be
saved even with empty device IDs. Invalid numeric edits remain visible and do not replace the
last valid preferences for that route; valid edits to the other route still save. Edits staged
while the route contains an invalid value are saved once it becomes valid again.

Saving and restoring never start audio. A running route's authoritative configuration is shown
when the WebView reconnects, but engine snapshots do not write user preferences. Metrics,
recovery and temporary bypass/degradation cannot overwrite them. Live denoise edits retain
their separate Apply command and engine confirmation. Stop restores the preferred processing
mode after temporary runtime bypass. Persistence errors never disable Stop.

The UI shows saving status and typed errors with retry. An unreadable settings file falls back
without automatic writes. The explicitly labeled Replace unreadable settings action authorizes
replacement with the current valid choices. Normal window Close continues hiding to the tray;
normal Quit drains accepted saves before exiting.

## Native storage and IPC

`ConfigStore` reads schema 1 from the application configuration directory. The document contains
only `schemaVersion` and `settings`, with fixed `microphone` and `incoming` route objects.
It rejects unknown schema/route fields, invalid roles/settings and IDs over 1024 bytes. Reads
are capped at 32 KiB. Empty/missing devices are configuration preferences, not an instruction
to open a stream; existing native Start validation remains authoritative.

`SettingsWriter` owns the file operations. Its control-plane mailbox retains one latest complete
document and at most one disk write in progress, independent of edit count. A fixed 150 ms
coalescing window combines rapid edits without allowing continuous typing to starve writes.
Writes validate the complete pair, write and sync a temporary file in the destination directory,
then atomically replace the file. No disk I/O holds the mailbox mutex. Failure retains the last
successful file and current in-memory choices; explicit retry submits the latest pair.

The two narrowly scoped commands are:

- `get_settings`: current accepted preferences, process-local `revision`, `savedRevision`,
  `loadWarning` and `writeError`.
- `save_settings`: `{ request: { settings, revision, replaceUnreadable } }`; validates and
  replaces the pending document, then returns an acceptance snapshot without waiting for disk.

Revisions are u32 values, initialized to zero on each process launch and never stored on disk.
The frontend initializes from the current accepted revision and sends a complete pair with each
new revision. Stale/reordered requests are rejected before mutation. A newer pair includes both
routes' current valid preferences, so rapid independent edits cannot lose the other route.
The UI observes durable completion separately, polling every 250 ms only while saves are pending.
Old replies cannot clear newer pending status; save/poll replies never refill the controls.

Normal Quit closes submissions, wakes the writer immediately, flushes the latest accepted
settings and joins it. A failed final save produces a non-successful shutdown result. An abrupt
process kill or an edit not yet delivered through IPC is outside the flush guarantee. PCM,
callbacks, DSP workers, DeepFilterNet ownership, AEC, stereo handling and route priority are
unchanged. No filesystem capability or generic file API is exposed to the WebView.

## Validation (2026-09-19)

| Check | Result |
| --- | --- |
| `cargo ci-test --locked` | Passed: 217 tests; 2 existing timing benchmarks ignored |
| Full workspace/all-target/all-feature Clippy, warnings denied | Passed |
| `cargo ci-fmt` | Passed |
| `npm run --prefix apps/desktop test` | Passed: 63 tests across 9 files |
| `npm run --prefix apps/desktop build` | Passed: TypeScript and production Vite assets |
| `npm run --prefix apps/desktop tauri -- build --bundles app -- --locked` | Passed: production macOS app |
| `git diff --check` | Passed |

Rust regressions cover round-trip persistence of both routes, incomplete/missing devices, AEC
off, bounded reads/IDs, invalid/corrupt/future files, explicit replacement, stale revisions,
500 edits during a blocked disk write with only the first and latest documents written,
normal Quit inside the coalescing window, failed writes and retry. The real Tauri invoke harness
checks preference restoration, desktop defaults, capability allowlisting and zero audio commands
caused by settings operations. Obsolete profile/migration tests were removed with that feature.

Frontend tests mount the actual React components in jsdom with a mocked metadata API. They
cover startup gating, automatic selection and exclusions, both routes after remount, missing
and reconnected devices, initialization while a route starts, invalid drafts, delayed replies,
durable save polling, failed reads/writes and retry, explicit replacement, temporary bypass,
Stop availability and immediate submission before window unmount. They do not claim native PCM
or acoustic acceptance. The native smoke test caught a generic Rust denoise default differing
from the existing desktop post-filter default; the implementation and IPC assertion now
explicitly preserve the desktop default.

Local ignored evidence is in `target/settings-*.log`, `target/settings-desktop.png`,
`target/settings-restored.png`, `target/settings-native-smoke.json` and the production app at
`target/release/bundle/macos/OpenKrisp.app`. The results above are local; GitHub CI is reported
separately on the pull request.

## Native smoke test and limitations

The production app was launched, quit normally, then reopened with a controlled saved file
containing a missing microphone, explicit AEC off, and 61/73 ms target latencies. Window capture
confirmed that the missing microphone stayed selected, AEC was off and the route remained
stopped. The saved file was byte-for-byte unchanged after relaunch. Normal Quit succeeded;
the test-created settings were then restored to autodiscovered devices, 40 ms target latencies,
AEC on for microphone and post-filter off for both routes. The app was left closed.

The first launch also connected to the audio engine, created the new settings file, selected
the Mac microphone and dedicated virtual endpoint, and displayed missing-driver guidance.
No audio was started. Native GUI edit-through-Quit automation was not completed; that interaction
is covered separately by the mounted frontend tests and the real Rust/Tauri persistence harness.

The named macOS drivers are not installed on this machine; this check does not qualify the
new-driver call path. macOS accessibility did not reliably expose the app window even while
CoreGraphics and window captures confirmed the visible WebView. This limited native click/edit
automation.

Linux desktop/hardware behavior is untested locally. Linux deterministic behavior is covered
by routing/selection tests, but remote CI and hardware acceptance remain separate. Phase 6's
two-hour concurrent runs, device recovery, Bluetooth profile changes, broader conferencing and
speakerphone acceptance remain open. The earlier Google Meet report remains macOS-only.
