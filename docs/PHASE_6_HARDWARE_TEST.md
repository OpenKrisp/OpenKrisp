# Phase 6 hardware acceptance

## Offline listening comparison

The `denoise-dfn` listening example processes a prerecorded fixture through the real model and
the independent-channel adapter. It does not open audio devices or record a microphone. Use
interleaved little-endian float32 PCM at 48 kHz, with one or two channels, finite samples within
±0.98, and a duration no longer than two minutes:

```sh
cargo run --release --locked -p denoise-dfn --example listening_comparison -- \
  INPUT.f32 OUTPUT.f32 2
```

The output must be a new file. The example flushes the model and trims its declared delay so the
input and output have the same duration. Convert both files to WAV using the same scale and
compare at a comfortable volume. Do not normalize each clip separately. Reported processing
times cover model inference only; this check does not exercise CoreAudio, BlackHole, resampling,
route scheduling, live bypass or end-to-end latency. Perceptual speech quality still needs a
listener, and live routing still needs the hardware checks below.

## Live acceptance

### Automated macOS listening run

With OpenKrisp Speaker installed and headphones connected, the native example drives the production
`AudioEngine` loopback route with a prerecorded fixture. Enumerate devices using
`cargo run --locked -p audio-cpal --example list_devices`, then supply the exact headphone output
ID and a stereo 48 kHz float32 LE fixture (1–60 seconds, finite samples within ±0.5):

```sh
cargo run --release --locked -p audio-engine --example macos_incoming_listening -- \
  INPUT_STEREO_48K.f32 'coreaudio:YOUR_HEADPHONE_OUTPUT_ID'
```

Run with normal macOS audio access, outside a filesystem-only sandbox; macOS may ask for audio
capture permission for OpenKrisp Speaker. The example explicitly opens OpenKrisp Speaker and the supplied physical
output. It does not change system defaults or open a physical microphone. Keep other applications
from playing into OpenKrisp Speaker during the check, because OpenKrisp Speaker mixes audio sent to it.

The sender writes stereo to native channels 1–2 and keeps channels 3–16 silent. The
updated example has been compiled; listening with the named driver remains pending.

The finite sequence plays the fixture at half amplitude in bypass, repeats it with speech
denoising, returns to bypass, and plays a left-only tone followed by a right-only tone. It records
route/device formats, model identity, metrics, mode changes, and cleanup. Ctrl-C cancels playback
and shuts down the engine. The listener must confirm audibility, speech quality and stereo order.
Metrics retain historical inference timings in bypass; increasing skipped-inference counts show
the bypass behavior. A successful signal check does not replace reviewing deadline/xrun counters
or the broader acceptance procedure below.

### Full hardware acceptance

Use headphones. Record OS, CPU, CPAL backend, exact devices, negotiated rates/channels, model
identity, route latency, p95/p99, queue fill, missed deadlines, overruns, underruns, resets and
degraded/skipped-inference counters. Retain the existing Phase 2–5 acceptance requirements.

1. On macOS install BlackHole 2ch using the documented setup and allow microphone access.
   Select BlackHole as a call/media application's output and physical headphones in Incoming
   audio. Keep a BlackHole virtual microphone route stopped. On Linux use an active PipeWire
   session, Start incoming audio, then select **OpenKrisp Incoming Audio** in the source app.
2. Start in bypass. Play speech, music, left-only/right-only audio and a duration marker fixture.
   Verify stereo order, duration after accounting for fixed latency, and absence of recursive
   capture. Enable speech noise reduction for a noisy speech fixture and listen for intelligibility
   and artifacts. Return music to bypass. Repeat at supported 44.1/48 kHz device configurations.
3. Start a microphone route concurrently. Linux can use its separate virtual microphone pair.
   macOS can use a physical microphone monitor with a distinct physical output; sharing BlackHole
   between both directions must produce an actionable rejection. Check Start, Stop and bypass on
   one route do not change the other. Headphone-output sharing must also be rejected.
4. Introduce temporary CPU load. Confirm incoming processing enters observable aligned bypass
   under microphone deadline/queue pressure, queues stay bounded, and the microphone remains
   intelligible. Remove load and confirm incoming recovers after quiet metrics without changing
   the user's selected mode. Record the hardware limits; the policy cannot guarantee scheduling.
5. Change system defaults while running. Explicit selected devices must remain selected. Remove
   and reconnect the selected output/capture endpoint. Loopback uses bounded headless recovery;
   native application PCM fails visibly and requires restart with a new source. No route may
   silently switch to another device. Test invalid and aggregate output requests.
6. Stop and Quit during startup, recovery and processing. Owned Linux incoming nodes must
   disappear after their streams stop; the other route's nodes must remain while it runs.
   Pre-existing external nodes and the installed BlackHole driver must remain installed.
7. Run two concurrent routes for at least two hours on built-in/USB hardware. Verify bounded queue
   fill, stable memory/thread counts, no increasing latency, and clean final shutdown. Test
   Bluetooth separately and record unsupported duplex profiles instead of inferring support.

The automated private PipeWire test establishes digital transfer, isolation and cleanup. It does
not establish acoustic quality, conferencing-app compatibility, CPU headroom or long-run hardware
stability. Open-speaker AEC acceptance belongs to Phase 7/E16.
