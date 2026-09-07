# Phase 3 Drift and Overload Implementation Report

## Scope

This change completes backlog epic E9 for the single-route headless engine. It adds adaptive
output-rate control for independent device clocks, bounded discontinuity handling, sustained model
deadline detection, and latency-aligned degraded bypass. It does not claim long-duration physical
hardware acceptance, live sleep/reconnect coverage, or the later multi-route engine service.

## Adaptive clock control

The worker now treats the configured output-latency target as an output-ring fill target. Once per
model frame, a filtered PI controller observes the ring occupancy and requests a relative output
resampling correction. Its initial tuning is deliberately conservative:

- correction is clamped to ±1,000 ppm;
- each model-frame update changes correction by at most 10 ppm;
- occupancy uses a low-pass filter before proportional and integral feedback;
- conditional integration prevents wind-up while the correction is saturated;
- a reset clears the filter, integral, and correction to nominal.

The output resampler uses Rubato's adjustable asynchronous sinc backend for all nominal rate pairs,
including 48 kHz to 48 kHz. This matters because equal advertised rates do not imply a shared
hardware clock. Ratio changes ramp over the next processing block and use the buffers allocated at
route startup; they do not insert or delete individual samples. The current signed correction is
published as `driftCorrectionPpm` and printed by the headless monitor.

## Discontinuity behavior

Input overflow/stale-discard and output-overflow operations already increment lock-free ring
discontinuity counters. The worker now observes those counters directly. On a new discontinuity it
resets both resamplers, the partial model-frame assembler, aligned dry delay, drift controller, and
the reducer's recurrent state, then targets the dry side of the existing equal-power crossfade.
Reset and error counters remain externally visible. The review-fix follow-up restores a prepared
pristine model state and coalesces audio accumulated during reset into one recovery discard;
reset duration is exposed separately. See `PHASE_0_4_REVIEW_FIXES.md`.

An output callback underrun continues to copy available audio, fill the remainder with silence, and
increment one underrun event per callback. It does not reset the model timeline by itself; the
adaptive controller responds to the lower occupancy, while sustained backend faults remain a
supervisor concern.

## Sustained overload behavior

Inference success and elapsed time feed a fixed-size, allocation-free deadline window. Four misses
within a full eight-model-frame window enter degraded mode; reducer failures also count as misses.
An isolated spike remains only a diagnostic deadline miss.

While degraded, every frame continues through the algorithmic dry delay, crossfade, limiter,
resampler, final limiter/meter, and bounded output ring, preserving the established timeline.
The review-fix follow-up skips 100 frames (one second at the current model hop), then resets once
and begins a contiguous four-frame recovery trial. Healthy inference below deadline leaves
degraded mode; a failed or late trial returns to cooldown. Reset duration is measured separately
from inference, so graph restoration cannot masquerade as a slow inference on every trial frame.
User bypass and disabled denoising continue to select dry output.

Snapshots and the CLI expose the current degraded flag, total overload entries, skipped inference
count, missed deadlines, and inference percentiles. Discontinuity recovery deliberately retains
the overload window so severe model pressure cannot repeatedly erase its own detection history.

## Deterministic evidence

Automated coverage includes:

- +100 ppm and -100 ppm source-clock simulations over 180 simulated seconds;
- +750 ppm and -750 ppm stress simulations over 240 simulated seconds;
- convergence after a simulated +100 ppm to -100 ppm clock-rate step;
- convergence near the configured fill target without reaching empty or capacity;
- correction sign, ±1,000 ppm bounds, 10 ppm/update slew, and reset behavior;
- adjustable equal-rate resampling producing the expected relative duration change;
- rejection of ratio requests outside the resampler's preallocated range;
- zero measured steady-state allocations while changing ratio and resampling;
- producer overflow detection followed by temporal DSP/model reset;
- one real delayed inference being recorded without entering degraded mode;
- sustained real delayed inference entering aligned dry bypass and skipping most model calls;
- deterministic degraded-mode probe cadence and recovery after healthy probes;
- serialization and aggregation of the new health metrics.

The complete locked workspace test suite, warnings-denied Clippy, formatting, frontend typecheck,
dependency-license gate, and offline DeepFilterNet spike pass locally. Pull-request CI remains the
cross-platform authority.

## Remaining Phase 3 acceptance

- Run independent physical input/output devices long enough to measure steady fill and correction.
- Exercise live disconnect/reconnect and sleep/resume on the supported hardware matrix.
- Tune controller and overload constants from recorded hardware traces if needed.
- Complete the two-hour soak and record baseline latency/inference measurements.
- Implement the multi-route engine command/snapshot service before the desktop control plane.
