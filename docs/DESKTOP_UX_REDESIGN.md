# Desktop UX review and redesign

## Audience and primary job

OpenKrisp helps people sound clearer in calls. The everyday job is to choose a
microphone, start it, and select OpenKrisp in a calling app. Cleaning up other
people's speech is a separate, optional job. Audio starts only on request.

## Review findings

- The original screen gives routing, processing, playback, setup, and diagnostics
  similar visual weight. The primary action appears after several technical decisions.
- “Route,” “endpoint,” “bypass,” and inference timings require knowledge of the
  implementation. A connected engine indicator does not establish that audio is on.
- Start, Stop, and Bypass appear together even when only one transport action applies.
- Setup guidance is below the controls, although choosing the right microphone in
  the calling app is essential to hearing the result.
- Device choices and automatic persistence already provide useful defaults. Preserve
  them, including missing-device selections, explicit starts, and authoritative status.

## Design plan

Palette: near-black `#08110F`, dark green surface `#0F1F1B`, soft white `#EFFCF8`,
muted sage `#A8BBB5`, mint `#63D9B3`, deep green accent surface `#142D24`.
Preserve the original black-and-green identity, as requested by the user. Use mint
for the primary action, switches, audio levels, and focus; reserve warm colors for
warnings and errors. Simplifying the workflow does not require changing the palette.

Type: the installed platform UI font for controls and body, with the rounded system
variant for the wordmark. A 12/14/16/20/28 scale keeps this a desktop utility rather
than a marketing page. No downloaded fonts or decorative display typography.

Layout: a compact, left-aligned, single-column window. A microphone medallion is
the sole expressive element; it connects the app's identity directly to its job.

```text
OpenKrisp                                  Settings

  (microphone)  Ready when you are
                Clearer speech for your calls.

Microphone                         [device picker]
Input level                         ▰▰▰▱▱▱▱▱▱▱
Noise reduction                              [on]
[                Start microphone               ]

In your call app
Microphone                    OpenKrisp Microphone
                                          Setup help

> Call audio                         Optional / On

Audio stays on this device.
```

The alternative, two equal microphone/speaker panels, makes the optional task
compete with the main one. Use a disclosure for call audio, with a persistent On
status when running. Put routing overrides, echo configuration, tuning, and
diagnostics in a separate Settings view. Keep settings mounted so navigation does
not discard pending changes or confirmation state.

## Review against the brief

An initial large status dashboard would retain the developer tool's information
hierarchy. Replace it with one primary transport action, one understandable noise
switch, and setup guidance beside the task. Avoid a decorative waveform: level
feedback must reflect actual microphone measurements. Avoid repeated cards and
marketing headlines; use a quiet dark-green surface for the call-app handoff.

## Interaction requirements

- Distinguish off, starting, running, reconnecting, stopping, failed, and disconnected.
  Never infer noise reduction from engine connectivity alone.
- Keep Stop accessible during startup/recovery and before saved settings finish loading.
- Keep missing devices selected; do not silently redirect audio.
- Noise reduction can be changed before Start and temporarily changed while running.
  Incoming audio defaults to unprocessed playback for music.
- Explain locked device choices and retain actionable setup/errors.
- Preserve echo-protection availability; do not imply that speakers are protected
  when the reference is missing.
- Support keyboard focus, reduced motion, narrow windows, and long device names.
- Validate command behavior and persistence with the existing integration tests;
  inspect stopped, running, settings, and narrow layouts in a browser when available.

## Implementation and validation

The window now defaults to 600 × 760, with a 360-pixel minimum width. Both stopped
and running microphone views fit the default content height. Settings keeps Stop
available beside the explanation for locked device controls. The call-audio
disclosure exposes its activity status even when collapsed.

The menu bar uses the same three-stroke mark as the desktop wordmark. Its 36-pixel
antialiased image supports an 18-point Retina menu bar, with macOS template tinting
for light and dark appearances and the mint accent on Linux.

The production frontend build and all 69 frontend tests pass. Added interaction
coverage verifies navigation and focus restoration, pre-start preferences, startup
cancellation, runtime confirmation, disabled-denoiser recovery, independent call
audio, and stale-meter clearing after a connection failure. Existing persistence,
device safety, diagnostics, and echo-status tests continue to pass.

Browser checks used simulated Tauri responses and inspected ready, running,
Settings, expanded call audio, missing-device, and 360-pixel layouts. No horizontal
overflow or browser exceptions were observed. The visual pass reduced vertical
spacing to fit the running view and removed the focus outline from the programmatic
heading focus target; interactive keyboard focus remains visible.

Native audio processing and physical call quality were not revalidated in this UI
change. No Rust audio code, routing rules, or IPC contracts changed.
