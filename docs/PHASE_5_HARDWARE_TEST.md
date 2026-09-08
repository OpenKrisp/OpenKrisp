# Phase 5 hardware acceptance

Status: **pending**. The development host does not have BlackHole installed. Automated graph and
lifecycle tests do not substitute for receiving-app audio acceptance.

The isolated Linux PipeWire cable test passes, but actual microphone/receiving-app tests remain
pending on macOS and Linux. Windows support has been removed; see [the removal decision](WINDOWS_SUPPORT.md).

## macOS setup

1. Build and install the combined OpenKrisp package using the [installer procedure](INSTALLATION.md).
   Restart when prompted. Verify a fresh install adds standard **BlackHole 2ch**, a reinstall
   reuses an existing same/newer driver, and removing the driver allows the installer to repair it.
   Source-build testing can use the [official BlackHole installer](https://existential.audio/blackhole/).
2. Use headphones. Leave macOS sound output and the receiving application's speakers set to
   headphones. Do not create an aggregate or multi-output route for this test.
3. Launch the desktop with `npm run --prefix apps/desktop tauri -- dev`, refresh devices, and confirm
   the setup panel shows **BlackHole 2ch · Ready**.
4. Choose **Virtual microphone**, a built-in physical microphone, and **BlackHole 2ch**. Start.
   Grant microphone access if requested. If denied, enable it in System Settings → Privacy &
   Security → Microphone, reopen OpenKrisp if macOS requires it, refresh, and start again.
5. In QuickTime Player choose **File → New Audio Recording**, open the input menu, and choose
   **BlackHole 2ch**. Keep live monitoring muted to avoid a second monitoring path. A conferencing
   app's microphone test with BlackHole 2ch is also suitable; do not send audio to other people.

## Record evidence

- Record a short sample with speech and representative background noise. Confirm input/output
  meters move, the recipient receives speech, and toggling bypass changes processing without
  changing duration. Listen to the recording after stopping the live route.
- Stop OpenKrisp while the receiver's meter is visible: after buffered audio drains, new microphone
  speech must no longer reach it. Restart ten times and repeat with Quit during Starting/Running.
  Verify no OpenKrisp route threads/streams remain after Quit; the user-installed endpoint should
  remain available.
- Restart OpenKrisp and confirm endpoint discovery still works. Change the system default output
  during the route: the selected destination must stay BlackHole. Change the default input and
  confirm the selected physical microphone remains the source.
- Repeat with a USB microphone. Disconnect/reconnect that microphone: recovery must be bounded,
  or the route must fail with a restart instruction. Refresh after an endpoint disappears: the
  setup panel must show missing/unavailable and prevent a new start until the endpoint returns.
- Confirm BlackHole inputs and aggregate sources are excluded from virtual microphone selection;
  the automated router tests additionally send those requests directly to the Rust boundary.
- Run for two hours with independent microphone and virtual endpoint clocks. Record start/end
  memory use, negotiated input/output formats, inference p99, underruns/overruns, drift correction
  and estimated latency. Queue fill and latency must not grow steadily.

Record macOS version, hardware, BlackHole version, receiving app/version, duration, observations
and any faults. Complete backlog E14-06 only once the receiving-app test has passed. Retain the
separate Phase 2/3 hardware matrix and soak criteria.

## Linux setup and additional checks

1. Install the OpenKrisp DEB/RPM with APT/DNF on a fresh desktop and confirm it installs
   PipeWire, `pw-loopback`, WirePlumber and PulseAudio compatibility. Log out and back in;
   verify the services start. Also test an existing PipeWire desktop and any supported
   PulseAudio migration. Launch OpenKrisp after the session is available; restart OpenKrisp
   if PipeWire was absent at launch.
2. Select a built-in/USB physical microphone and **OpenKrisp Microphone** as the virtual destination.
   A missing endpoint should say **Created when you start** and permit Start. Starting creates
   `openkrisp_sink` and `openkrisp_source`, and the receiving app selects **OpenKrisp Microphone**.
3. Repeat the speech/bypass/listening, USB reconnect, default changes and two-hour checks above.
   Stop/Quit must remove OpenKrisp-owned nodes, and the receiving app must stop receiving live
   microphone audio. A normal restart recreates them without duplicates. Externally owned nodes
   are not removed by OpenKrisp.
4. Stop while Starting and close the app while Starting/Running; verify its `pw-loopback` process
   and owned nodes disappear. Kill that owned process in a test session: the route must fail
   with an actionable error and stop microphone capture; starting again must recreate it.
5. Verify sink monitors and virtual/unknown inputs cannot be selected as microphones. Check
   PulseAudio-only/ALSA-only sessions show an unsupported virtual endpoint with setup guidance.

Record distro, PipeWire/session-manager versions, kernel/audio devices, receiving app, negotiated
formats, and evidence. The CI cable test uses only a private graph and synthetic samples; it does
not establish microphone classification, noise-reduction quality or conferencing compatibility.
