# Phase 5 macOS and Linux virtual microphones

## Result and support boundary

OpenKrisp targets macOS and Linux only. Windows support, its adapter, build integration and CI
job have been removed. The [removal decision](WINDOWS_SUPPORT.md) and
[historical candidate evaluation](VIRTUAL_CABLES_EVALUATION.md) record why.

| OS | Backend | Current support |
| --- | --- | --- |
| macOS | BlackHole 2ch, GPL-3.0 | Existing driver integration |
| Linux | PipeWire, MIT | Native CPAL backend; automatic endpoint creation and cleanup; isolated live PCM test passes |

Physical microphone, receiving-app and long-run acceptance remains required on both platforms.
The shared microphone/denoise/CPAL pipeline and Tauri command boundary are reused.

## Implementation

`platform-routing::current_router` selects BlackHole on macOS and `linux::LinuxRouter` on Linux.
The macOS adapter remains gated by `macos-blackhole`; disabling it retains physical monitoring.
The desktop and audio I/O reject build targets other than macOS and Linux.

- macOS uses the established [BlackHole 2ch](https://github.com/ExistentialAudio/BlackHole) endpoint.
  Discovery uses native CoreAudio identity and transport metadata.
- Linux uses [PipeWire loopback](https://docs.pipewire.org/page_module_loopback.html) through
  [`pw-loopback`](https://docs.pipewire.org/page_man_pw-loopback_1.html). The logical destination
  is `linux:pipewire`; CPAL IDs are `pipewire:openkrisp_sink` for processed playback and
  `pipewire:openkrisp_source` for the receiving app's microphone.
- Linux discovery rejects incomplete or ambiguous pairs, incorrect roles, non-stereo formats and
  unsupported hosts. Both endpoints form one recursive path. Virtual, aggregate and unclassified
  sources are rejected; Linux sink monitors cannot become microphone sources.
- Logical destinations and explicit native IDs are revalidated for every stream generation.

Linux enables CPAL 0.18.2's `pipewire` feature. Native bus metadata classifies physical devices;
where ALSA nodes omit bus metadata, numeric ALSA card addresses resolve through sysfs to
recognized hardware buses. Unknowns stay unknown. The app's sink/source IDs are virtual.

`AudioDeviceDescriptor.driver` and `PlatformEndpoint.canCreate` remain additive metadata with
Rust serde defaults. The setup UI uses endpoint-provided names/instructions and permits creation
only for a missing creatable endpoint. The eleven-command Tauri allowlist is restricted to macOS
and Linux. PCM never crosses IPC or a subprocess pipe.

## Linux lifetime and recovery

Starting a missing Linux virtual route validates the physical source before launching the fixed
`pw-loopback` command on the route owner. It uses no shell, caller-supplied command, system default
changes or persistent configuration. Endpoint discovery has a three-second readiness budget and
cancellable waits; native enumeration itself retains CPAL's bounded timeout. Initialization
failure/cancellation drops and reaps the child. The child runs as the same user as OpenKrisp.

The active route retains the child through stream recovery. Stop/Quit tears down streams before
terminating the child; its nodes disappear with the client. An unexpectedly exited child stops
capture and produces an actionable routing fault. A new Start recreates the pair. Pre-existing
valid endpoints are reused and not removed; ambiguous or partial existing pairs are rejected.
Unrecoverable native stream/device changes retain bounded failure/restart behavior.

Linux also arms `PR_SET_PDEATHSIG(SIGKILL)` before executing the loopback client and checks
that its parent did not disappear before the signal was armed. Process exit, SIGTERM and
SIGKILL therefore remove the owned client even when Rust destructors do not run. The kernel
ties this to the spawning thread; the engine creates and retains each session on its route
owner thread. This is a small, documented unsafe boundary in `linux_process`; the rest of
`platform-routing` denies unsafe code. Shared PipeWire services and external endpoints are
unaffected.

After recovery exhaustion, Stop remains available to release any resources retained by the
failed route. Start can still replace the failed route. CPAL's `pipewire:input_default` and
`pipewire:output_default` aliases are excluded from the selectors and rejected by backend
route validation, including physical monitoring. Stored routes must select concrete IDs so
recovery cannot follow a changed system default to speakers or another microphone.

The receiving app selects **OpenKrisp Microphone after Start**. PipeWire and a session manager
(normally WirePlumber) must already be running, and `pw-loopback` must be on PATH. Apps using the
PulseAudio API need `pipewire-pulse`. CPAL caches initial socket discovery, so restart OpenKrisp
if PipeWire was unavailable at launch. ALSA/PulseAudio-only sessions cannot expose this virtual
microphone. Refresh devices to update the setup panel after graph changes.

Debian/RPM bundles now declare the full PipeWire runtime, tools, WirePlumber and PulseAudio
compatibility dependencies. A combined macOS package builder includes BlackHole and its source/license.
AppImage/DMG are no longer default artifacts. Source builds still require manual dependencies.
See [installer implementation and release constraints](INSTALLATION.md); clean desktop
installation acceptance and the BlackHole integration licensing decision remain pending.

## Validation

The retained automated routing tests cover native identity, ancestry, incomplete/ambiguous
endpoints, formats, recursion, missing hosts, process errors and cleanup. Frontend tests cover
setup states, selection validation and the existing route lifecycle. CI targets macOS and Linux.

After Windows removal, the native ARM64 Debian 12 container passes audio/router tests and
warnings-denied audio/router/engine Clippy, including all targets. PipeWire 0.3.65 and WirePlumber
0.4.13 transfer a known two-channel signal through actual CPAL callbacks. Two creation, transfer
and teardown cycles pass using `linux::LinuxRouter`.

The live test uses a private PipeWire socket and D-Bus session with silent virtual fixtures;
it never connects to the user's graph. It verifies both PCM channels, masks sink-monitor input
exposure and checks that owned endpoints disappear. On WirePlumber 0.4 its private test
configuration disables the Bluetooth/logind component, which cannot initialize without seats.

The review follow-up passed 74 affected Rust tests on macOS, 21 frontend tests and the
frontend build. Native Linux audio/router tests passed, including a subprocess regression
for destructor-free exit, SIGTERM and SIGKILL. The private PipeWire test now also kills the
owning process with SIGTERM/SIGKILL, waits for node removal, then recreates and drops an owned
route. Workspace Clippy on macOS and audio/router/engine Clippy on Linux passed with warnings
denied. A failed-route supervisor regression verifies Stop releases the retained owner once.

Reproduce on Linux with PipeWire, WirePlumber, D-Bus, the Rust toolchain, `libasound2-dev`,
`libpipewire-0.3-dev`, `libclang-dev` and `pkg-config`:

```sh
dbus-run-session -- bash scripts/test-linux-pipewire.sh
```

Native macOS/Linux receiving-app and microphone tests follow
[the hardware acceptance procedure](PHASE_5_HARDWARE_TEST.md); E14-06 remains open. Automated
results do not establish hardware latency, acoustic quality or two-hour stability.
