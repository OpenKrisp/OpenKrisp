# Desktop UX review and redesign

## Audience and primary job

OpenKrisp helps people hear and be heard clearly in calls. Reducing noise around
your microphone and reducing noise from other people's voices are equally visible
core functions. Each starts independently and only on request.

## Product decisions

The desktop should make engineering choices for the user. Grouping technical
controls under Advanced audio still exposes decisions unrelated to making a call.
Remove that section, including manual virtual-device routing, microphone monitoring
mode, target latency, reduction limits, post-filter, the echo toggle, duplicated
processing selectors, and live diagnostic tables. Keep engine capabilities and
validation in the underlying APIs and development tools.

The user chooses their physical microphone and playback device. The app
selects its dedicated virtual microphone and speaker, uses the existing 40 ms target
latency and denoiser defaults, and enables microphone echo cancellation. Starting
either microphone or call audio enables speech noise reduction with one action.
Call audio is for voices; music should play directly through the usual output.
These are fixed product defaults, not a claim of newly implemented adaptive tuning.

Existing physical-device selections, including unavailable devices, are preserved.
Retired technical preferences resolve to the defaults for the next start. Active
routes continue with their actual configuration until stopped. Ambiguous virtual
device discovery blocks Start rather than guessing a destination. Missing virtual
devices retain installation and repair guidance.

## Visual direction

Preserve the requested black-and-green identity: near-black #08110F, dark green
surface #0F1F1B, soft white #EFFCF8, muted sage #A8BBB5, mint #63D9B3 and deep green
accent surface #142D24. Use the platform UI font and a 12/14/16/20/28 scale.

Use a 1080 × 720 default window with two equal columns, 24px between them and 20px
of inner padding. The matching panels have 20px titles, 12px descriptions and
independent status indicators. Your microphone reduces noise around you; Your speakers reduces background noise from other people in your call. Both panels
are always expanded. Below 860px, stack them with identical visual weight. Help
uses matching connection guides for both routes. Reuse the established palette and font.

The illustrated audio journeys are the expressive focal point: microphone or
call app, OpenKrisp sound filter, then the receiving device. Directional paths explain the flow;
state captions reflect confirmed processing, bypass, degradation and disconnection.
An existing monitoring route shows headphones until stopped. Avoid artificial meters
or continuous waveform animation.

Help uses the same palette and platform typography, with left-aligned content in
two distinct sections. Matching microphone and speaker connection guides sit side
by side, each with three numbered steps and the actual call-app device name. On
narrow windows they stack. Troubleshooting remains below the guides. Keep 20px
panel padding, a 28px gap between sections and smaller supporting text. The speaker
guide explains where to listen and how to switch back before stopping. Device
installation problems belong in the guide for the affected device. Shorten the
window itself to remove excess space while retaining the established text sizes.

Avoid giving every action its own equal card: connecting is the primary task, while
refreshing devices and copying support details are secondary recovery actions.
Keep these actions visible with short explanations, without adding options.

Your speakers mirrors the microphone flow inside its own equal panel:
call app → OpenKrisp → headphones or speakers. Reuse the mint filter illustration,
sage line work, platform type scale and three-column layout. Draw a call window
with voice waves at the source and playback devices at the destination. Keep the
device picker and main action together; put call-app connection instructions in a
separate green panel below. The exact OpenKrisp Speaker name belongs in that panel,
where the user is told to select it. Avoid a second paragraph explaining the same
flow, technical status tables, or nested settings groups. Move the music advice
into the speaker connection guide to keep both sets of everyday controls aligned.
Remove the old microphone-only global status headline; show status within each
panel so running one direction cannot imply the other direction is on.

```text
Your speakers                         Off / On
[Your call app] → [OpenKrisp] → [You hear]
Play through                          [device picker]
Playback level
[                 Turn on OpenKrisp                 ]
┌ In your call app, choose this speaker: ─────────────┐
│ OpenKrisp Speaker                                  │
│ > How to connect                                   │
└────────────────────────────────────────────────────┘
```

```text
OpenKrisp                                      Help
┌ Your microphone       Off ┐  ┌ Your speakers       Off ┐
│ Reduce noise around you.  │  │ Reduce other people's     │
│                          │  │ background noise.         │
│ Mic → OpenKrisp → Call    │  │ Call → OpenKrisp → You    │
│ Microphone      [device] │  │ Play through     [device] │
│ Input level              │  │ Playback level            │
│ [Turn on OpenKrisp]      │  │ [Turn on OpenKrisp]        │
│ Call app microphone:     │  │ Call app speaker:         │
│ OpenKrisp Microphone     │  │ OpenKrisp Speaker         │
│ > How to connect         │  │ > How to connect          │
└──────────────────────────┘  └───────────────────────────┘
Private by design. Audio is processed on your device.
```

```text
Help                                            Back

┌ Connect your microphone ─────┐  ┌ Connect your speakers ─────────┐
│ 1. Choose your microphone.   │  │ 1. Choose where to listen.     │
│ 2. Turn on OpenKrisp.        │  │ 2. Turn on OpenKrisp.          │
│ 3. In your call app, select  │  │ 3. In your call app, select    │
│    OpenKrisp Microphone.     │  │    OpenKrisp Speaker.          │
│ Keep your usual output for  │  │ Switch back to your usual     │
│ microphone-only cleanup.   │  │ output before turning off.    │
└─────────────────────────────┘  └───────────────────────────────┘

Troubleshooting
Missing a device?                    [Refresh devices]
Reconnect it, then refresh the list.
──────────────────────────────────────────────────────
Still need help?                [Copy support details]
Copy details to share with support.
No recordings or device names.
```

## Interaction requirements

- Every microphone or call-audio start enables noise reduction. Each has one
  Turn on OpenKrisp / Turn off OpenKrisp action, with no separate noise-reduction
  switch. The section identifies which audio the action controls.
- Keep Stop accessible during startup, recovery, pending call-audio changes and settings
  loading. Navigation keeps controllers mounted and preserves pending commands.
- Show exact call-device names. Missing devices lead to actionable connection help.
- Echo cancellation runs when its playback reference is available. The connection
  guide recommends headphones without exposing technical echo-cancellation status.
- Generate sanitized support details only on request. Copy directly; if clipboard
  access fails, show a selected, frozen report for manual copying. Send nothing.
- Maintain keyboard focus, reduced-motion behavior, narrow-window wrapping and
  readable long device names. Restore focus to Help when returning with Escape.

## Validation

All 71 frontend tests pass. Coverage includes default settings and legacy migration,
physical-device persistence, missing/ambiguous endpoints, startup cancellation,
Stop during call-audio startup/recovery, confirmed flow state, independent call audio,
connection loss, support-report privacy and manual copying, persistence retries,
and unchanged live route configuration. Retired controls are absent from the DOM.
The frontend typecheck and macOS app bundle build pass.

The updated app in /Applications was opened and inspected on macOS. The home page
shows both routes off without starting audio. Help fits the shorter 720px-high window
with two matching connection guides and a Troubleshooting section. At the minimum
360px width, the guides stack and wrap cleanly, with troubleshooting buttons below
their explanations. Each guide shows three connection steps and the correct call-app
device name, followed by Refresh devices and Copy support details. No microphone
comparison, Advanced audio section, numeric tuning controls or live diagnostic tables remain.

Both audio panels were inspected at the default 1080 × 720 window size, with their
illustrations, device pickers, meters, on/off actions and connection guides visible
without scrolling. At the minimum 360px width, they stack and remain expanded with
no horizontal overflow. Help and Escape navigation preserve both panels and their
device choices. The app remained idle during visual validation. Frontend
tests cover speech cleanup on start, old bypass preferences, unchanged active
routes, start failures/cancellation, recovery, degraded processing and disconnected
meters. Microphone and call audio remain independently controllable.

The navigation and page heading use Help with a circled-question-mark icon. There
is no saved-settings footer on this page; physical device choices still save
automatically, and persistence errors remain actionable when they occur.

Native audio processing and physical call quality were not revalidated in this UI
change. No Rust audio code, routing safety checks or IPC contracts changed.
