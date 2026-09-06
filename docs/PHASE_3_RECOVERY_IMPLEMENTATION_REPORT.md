# Phase 3 Route-Recovery Implementation Report

## Scope

This change is the first focused slice of roadmap Phase 3. It completes backlog item E8-06 for the
single-route headless monitor: startup stabilization, portable stream-fault classification, bounded
route reconstruction, and recovery diagnostics. It does not claim the rest of Phase 3. Adaptive
clock-drift control, sustained-overload degradation, the multi-route engine command/snapshot
service, live reconnect acceptance, and long soaks remain open.

## Startup stabilization

Route construction now follows this sequence:

```text
resolve input and prefer its current native format
  -> build/play input with PCM transfer gated
  -> require callbacks through a settling interval
  -> re-enumerate output and prefer its post-capture native format
  -> build/play a silence-gated output candidate
  -> require callbacks through a settling interval
  -> construct and warm the worker
  -> enable bounded PCM transfer and verify the complete route
  -> Running
```

The callback gates are atomics. The input callback receives its eventual worker notification through
an `OnceLock`; neither gate adds a callback lock, allocation, sleep, log, or model operation.
Candidate output callbacks render silence until the worker and prefilled output ring are ready.

Input activation can change a headset's output profile. The output stage therefore creates a fresh
host/device handle and supported-format view on every attempt while retaining the active input
stream. It uses the endpoint's current default rate and channel count as negotiation preferences and
lets the existing graph resamplers bridge that native format to the canonical 48 kHz model rate.
This avoids changing a shared hardware clock solely to match the model.

## Recovery policy and lifecycle

`RouteRecoveryPolicy` provides a validated, finite attempt count, capped exponential backoff,
activation timeout, settling duration, and polling interval. Errors are reduced to four supervisor
actions:

- `Continue` for observable conditions that leave the stream usable, including xruns;
- `Reconfigure` for invalidated or unsupported stream configurations;
- `Retry` for transient device, host, busy, resource, or backend failures;
- `Fail` for permission, invalid-input, unsupported-operation, and unknown fatal failures.

The supervisor checks terminal issue categories and unexpected worker exit during low-rate health
polling. A recoverable fault transitions the route from `Running` to `Reconfiguring` or `Recovering`,
releases the old generation in deterministic input/worker/output order, and rebuilds with the same
device selections, denoise settings, and bypass state. Success returns to `Running`; exhaustion or a
fatal error transitions to `Failed` without an unbounded retry loop.

Snapshots preserve callback and issue counters across generations and expose total recovery
attempts, successful recoveries, the last recovery error, and the triggering input/output issue.
The CLI prints recovery counts and includes the retained backend diagnostic on terminal failure.
CPAL adapter errors now preserve the platform's textual detail in addition to their portable kind.

## Deterministic evidence

Automated coverage includes:

- bounded and capped recovery-budget iteration;
- portable classifications for reconfigure, retry, continue, and fatal conditions;
- rejection of zero or incoherent recovery timing configuration;
- legal `Running -> Reconfiguring -> Recovering -> Running` transitions;
- terminal-category retention when later non-terminal events arrive;
- saturating accumulation of callback and stream diagnostics across generations;
- native endpoint-rate/channel selection while preserving a requested buffer size;
- retention of the platform message in typed CPAL errors.

The standard warnings-denied Clippy and locked workspace test gates pass. Cross-platform compilation
remains enforced by the pull-request Linux, macOS, and Windows jobs; the physical evidence below is
specific to the primary macOS development host.

## Physical smoke evidence

On 2026-09-04, a five-second MacBook microphone-to-speakers run negotiated 48 kHz mono input and
48 kHz stereo output. It stopped cleanly after 559 input and 499 output callbacks, with non-zero
microphone levels and zero stream issues, terminal issues, overruns, underruns, discontinuities,
deadline misses, model errors, or recoveries.

The connected AirPods initially reproduced the prior duplex failure. After capture activation,
CoreAudio advertised a 24 kHz stereo default output, but the old selection requested 48 kHz. CPAL's
preserved backend detail reported `Sample rate update timed out`, followed by
`DeviceNotAvailable`. Retrying the whole generation or retaining capture while repeating that 48 kHz
request did not succeed.

After native-format negotiation was added, the same AirPods passed both routes:

- default input/output: five seconds at 24 kHz mono input and 24 kHz stereo output, 314 input and
  298 output callbacks, non-zero microphone levels, and no reported faults, overruns, underruns,
  deadline misses, model errors, or recoveries;
- explicit stable input/output IDs: three seconds at the same formats, 213 input and 196 output
  callbacks, non-zero microphone levels, and the same zero-error result.

These runs confirm that local AirPods microphone capture, output callbacks, model inference, and
clean shutdown operate together. They do not establish subjective cleaned-audio quality: the runs
were quiet and the output meter remained at zero after denoising. They also do not substitute for a
physical disconnect/reconnect test or the two-hour soak.

## Remaining Phase 3 work

- Exercise disconnect/reconnect and sleep/resume on the supported hardware matrix.
- Complete the two-hour queue/latency soak and baseline measurements.
- Add the multi-route engine command and snapshot service before the desktop control plane.

E9 adaptive drift, discontinuity reset, and overload behavior was completed in the subsequent
slice documented by `PHASE_3_DRIFT_OVERLOAD_IMPLEMENTATION_REPORT.md`.
