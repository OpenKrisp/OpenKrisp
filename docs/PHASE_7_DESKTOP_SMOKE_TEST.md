# Phase 7 desktop smoke test

## Result and environment

On 2026-09-09, the actual Tauri desktop app launched on macOS 26.6.2 (25G83) and
passed the route/control checks below. Both debug and optimized native builds were exercised.
The optimized microphone run recorded **one missed processing deadline**. No sampled route
reported audio overruns, underruns, native stream issues, callback violations or model errors.
This is a short functional smoke test, not performance or full hardware acceptance.

The microphone route used the built-in microphone at 48 kHz mono and BlackHole 2ch at
48 kHz stereo. The incoming route used BlackHole at 48 kHz stereo and Pioneer Speakers at
44.1 kHz stereo. Both directions negotiated 512-frame buffers. AirPods/headphones were not
enumerated; the incoming test used silence and establishes no listening-quality result.
System audio defaults were not changed.

## Method and checks

The window was visually inspected. Temporary development instrumentation clicked the actual
rendered controls inside the Tauri WebView, then checked real Rust engine snapshots through
the existing metadata API. This was not a browser-only mock. A separate temporary native Rust
thread consumed the new microphone capture/render readers; it inspected PCM in Rust and wrote
only counters/timestamps to the local test log. No PCM crossed IPC or was recorded to disk.
The optimized runs used `tauri dev --release --no-watch`, so the frontend still came from Vite.
All instrumentation and its temporary dependency were removed afterward.

After restoration, `npm run --prefix apps/desktop tauri -- build --no-bundle -- --locked`
also passed. The resulting `target/release/openkrisp-desktop` launched with the embedded
production frontend, and its rendered window was visually checked. It was left open with audio
stopped. This final check establishes clean production startup; the route/control sequence above
was exercised in the optimized development build.

| Check | Result |
| --- | --- |
| Launch, render and engine connection | Passed; real device inventory and connected status |
| Refresh/select built-in microphone and BlackHole endpoint | Passed |
| Start microphone, denoise for 5 seconds, bypass for 3 seconds, re-enable, Stop | Passed; nonzero input/output signal |
| Restart microphone, run for 3 seconds, Stop | Passed; new stream identities |
| Reject simultaneous BlackHole virtual microphone and BlackHole incoming source | Passed; incoming Start disabled with actionable explanation |
| Stop microphone and release the incoming control | Passed; incoming Start enabled |
| Stop while microphone is Starting | Passed; reached Stopped and did not revive during the following second |
| Incoming stereo silence: Start, 5 seconds bypass, 5 seconds speech mode, return to bypass, Stop | Passed at 48 → 44.1 kHz; zero signal and no errors/xruns/deadlines |
| Quit with an active microphone route | Passed; native `AppHandle::exit(0)` entered the normal shutdown handler and the process exited with status 0 |

Quit was invoked through the same native exit API used by the tray; the tray menu interaction
itself was not verified. Profile editing/persistence, conferencing applications and device
removal were not exercised by this smoke test.

## Optimized microphone observations

These are maxima of snapshots sampled every 500 ms, not a latency measurement or exhaustive
trace. Deadline counts are cumulative within one route generation; restart resets them.

| Observed phase | Maximum input/output peak | Maximum reported inference p99 | Missed deadlines |
| --- | --- | --- | --- |
| Denoise, 5 seconds | 0.0512 / 0.0547 | 5.122 ms | 0 |
| Bypass, 3 seconds | 0.0869 / 0.0744 | 6.124 ms | 1 |
| Restart, 3 seconds | 0.0411 / 0.0231 | 4.951 ms | 0 |

The existing physical-microphone bypass keeps model processing warm. Its timing counters remain
meaningful in bypass; the missed deadline must not be dismissed because the UI was in that mode.
The initial debug run also recorded one missed deadline. There was no controlled comparison
against Phase 6, so these runs neither attribute the misses to the taps nor establish adequate
CPU headroom. A quiet, longer run with the ordinary optimized app remains necessary.

## Native reference observations

The first optimized microphone generation supplied 1,666 blocks per direction; its restart
supplied another 648 per direction. All inspected samples were finite, and the mono microphone's
stereo output reference had equal left/right samples. The consumer observed no tap overflow or
unknown timestamp. Each direction had the expected initial discontinuity and no subsequent gap
in these runs. The short conflict check also supplied 180 blocks per direction without loss.

At each observed Stop, retained blocks became invalid and cached delay estimates returned
`None`. Restart replaced stream IDs 1/2 with 3/4. These are live checks of generation/lifecycle
handling; exact post-conversion sample equality, asymmetric stereo channel order, underrun
content and intentional overflow remain covered by deterministic tests.

The reported capture age was 11.708333 ms and predicted render lead was 10.666667 ms, summing
to **22.375 ms of estimated I/O delay**. This excludes worker/model queues and acoustic or
unreported device/transport delay. It is not measured end-to-end latency. Short-window fitted
rate differences fluctuated with callback timing; independent-clock accuracy is established only
for the deterministic fixtures so far. No incoming-route tap consumer was attached during the
silent incoming test, which also exercised normal playback with an unconsumed bounded tap.

## Evidence and remaining acceptance

Ignored local artifacts under `target/phase7-desktop-smoke/` retain `report.jsonl`, `summary.json`,
`native-reference.log`, and the temporary test scripts for inspection. Launch/build logs are
`target/phase7-desktop-dev.log` and `target/phase7-desktop-release.log`; the initial window capture
is `target/phase7-desktop.png`. These artifacts are local evidence, not repository fixtures.
The clean production build/launch logs are `target/phase7-desktop-production-build.log` and
`target/phase7-desktop-production.log`; its window capture is
`target/phase7-desktop-production.png`.

The requested Rust/frontend tests, formatting, warnings-denied Clippy and frontend production
build were rerun after restoring the normal app files; see the
[implementation report](PHASE_7_RENDER_REFERENCE_IMPLEMENTATION_REPORT.md) for the complete
software validation results.

Still open: Phase 7 incoming listening/stereo perception with headphones, two-hour concurrent
hardware runs, measured latency and clock accuracy, physical device recovery, Bluetooth profile
changes, CPU-load acceptance, conferencing compatibility, and installer/package acceptance.
Phase 6's short BlackHole → AirPods listening result remains its existing limited evidence.
AEC selection, integration and open-speaker acceptance remain subsequent E16 tasks.
