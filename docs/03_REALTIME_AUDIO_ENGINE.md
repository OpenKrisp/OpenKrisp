# Real-Time Audio Engine

## 1. Purpose

The real-time audio engine owns all timing-sensitive work between CPAL endpoints and the DeepFilterNet adapter. It must tolerate arbitrary callback sizes, mismatched device formats, independent device clocks, temporary model slowdowns, and device lifecycle events.

## 2. Thread model

A route normally uses these execution contexts:

| Context | Owned work |
|---|---|
| Audio supervisor thread/task | Route creation, teardown, reconfiguration, recovery |
| CPAL input callback | Convert device samples to `f32`, enqueue, update counters |
| DSP worker thread | Resample, frame, infer, crossfade, limit, meter, produce output |
| CPAL output callback | Dequeue, convert to device type, fill silence/fallback on underrun |
| Metrics publisher | Read snapshots and emit low-rate events |

The operating-system audio backend may choose its own callback threads. Code must not assume input and output callbacks run on the same thread or at synchronized times.

## 3. Callback contract

### Required behavior

- Use only preallocated state captured by the callback.
- Convert sample formats with inline arithmetic or library traits that do not allocate.
- Copy samples in contiguous chunks where possible.
- Use non-blocking bounded queues.
- Update atomic counters using relaxed ordering where sufficient.
- Return quickly even when downstream processing is unavailable.

### Forbidden behavior

- DeepFilterNet inference.
- Resampler construction.
- File or network I/O.
- Tauri IPC.
- Waiting on a channel.
- Taking a contended mutex.
- Normal `tracing` or `println!` calls per callback.
- Allocating a new `Vec`, `String`, or error object in steady state.
- Retrying until a queue operation succeeds.

## 4. Queue topology

Use one single-producer/single-consumer ring for each callback boundary:

```text
Input callback -> input SPSC ring -> DSP worker
DSP worker -> output SPSC ring -> output callback
```

Recommended properties:

- Capacity expressed in audio frames, not ambiguous scalar samples.
- Capacity allocated during route startup.
- Slice-based push/pop APIs when available.
- Explicit metrics for current fill, high-water mark, dropped frames, and underruns.
- No dynamic growth.

### Initial sizing

Start with:

- Input capacity: approximately 100 ms.
- Output capacity: approximately 100-200 ms.
- Normal target output fill: approximately 20-30 ms.
- Startup prefill: enough to cover model latency plus a small safety margin.

These are initial tuning values, not fixed product guarantees. The final values must be measured on each platform.

## 5. Audio frame terminology

Use precise type names:

- **Sample:** one scalar value for one channel.
- **Audio frame:** one sample for every interleaved channel at one instant.
- **Device buffer:** the arbitrary number of audio frames delivered by CPAL.
- **Model frame:** the exact sample count expected by one DeepFilterNet processing call.

Avoid using `frame` for both an interleaved audio frame and a model block without qualification.

## 6. Format normalization

The engine should normalize device data in stages:

1. Convert CPAL sample type to normalized `f32`.
2. Preserve the source channel count long enough to apply an explicit mapping policy.
3. Convert to the route's canonical channel layout.
4. Resample to the model rate.
5. Assemble exact model blocks.

For a microphone route, the initial canonical model layout can be mono. For stereo speaker speech, evaluate whether the chosen DeepFilterNet adapter supports independent channels or multi-channel processing. Do not silently average stereo speech without documenting the behavior.

## 7. Frame assembly

CPAL callback sizes do not have to match model block sizes. The DSP worker needs a preallocated frame assembler:

```rust
pub struct ModelFrameAssembler {
    channels: usize,
    frame_samples_per_channel: usize,
    pending: Vec<f32>,
    pending_len: usize,
}
```

The actual implementation should avoid moving the entire pending buffer on every frame. A circular buffer or indexed scratch region is preferable.

Requirements:

- Accept any positive chunk length.
- Produce zero or more complete model frames.
- Preserve ordering and channel alignment.
- Keep incomplete trailing samples for the next iteration.
- Reset cleanly after discontinuity.
- Never allocate after initialization.

## 8. Resampling

The model rate is treated as canonical, expected to be 48 kHz for the selected DeepFilterNet model. Device rates may be 44.1, 48, 88.2, 96 kHz, or other values.

Use two conceptual resamplers when needed:

```text
source device rate -> fixed model rate -> destination device rate
```

Requirements:

- Construct and preallocate resamplers during startup.
- Keep resampling in the DSP worker.
- Support variable input chunk sizes through an adapter if the chosen resampler expects fixed blocks.
- Report group delay as part of the latency estimate.
- Reset or flush after major discontinuities.

## 9. Independent clocks and drift control

Input and output devices have independent hardware clocks. Even if both report `48_000`, a small rate difference can empty or fill the output queue over time.

Use output-ring occupancy as the feedback signal for a slow adaptive correction:

```text
fill_error = target_output_fill - current_output_fill
integral = clamp(integral + fill_error)
correction_ppm = clamp(kp * fill_error + ki * integral)
adaptive_ratio = nominal_ratio * (1 + correction_ppm / 1_000_000)
```

This sign assumes the backend ratio is output rate divided by input rate, so a positive correction
produces more output frames. Invert the correction when a backend defines its ratio oppositely.

Design constraints:

- Apply corrections gradually.
- Bound the correction to a conservative parts-per-million range.
- Reset the controller after route rebuilds.
- Record current correction and queue fill in metrics.
- Avoid routine single-sample duplication or deletion because it can create audible discontinuities.

If source and destination share a hardware clock, the controller should settle near zero correction.

## 10. Overflow and underrun policies

### Input overflow

The engine must prefer low bounded latency over preserving stale audio.

Recommended policy:

1. Increment `input_overruns`.
2. Drop the oldest complete processing block or enough oldest audio to return below a target fill.
3. Mark a discontinuity.
4. Reset model and resampler state if the gap is large enough to invalidate temporal state.
5. Crossfade from aligned dry or silence when resuming.

### Output underrun

Recommended policy:

1. Write available processed samples.
2. Fill the remainder with silence or a preselected aligned fallback.
3. Increment `output_underruns` once per callback, not once per sample.
4. Notify the supervisor only when underruns exceed a threshold or form a sustained pattern.

### Sustained overload

If the worker repeatedly misses deadlines:

- Reduce optional DSP work.
- Prefer preserving the microphone route over speaker enhancement.
- Increase the target output buffer within the configured latency ceiling.
- Enter latency-aligned bypass if model processing remains unstable.
- Surface a clear degraded-state metric.

## 11. Latency-aligned bypass

A raw dry signal arrives earlier than the DeepFilterNet output. Switching directly between them creates a timing jump and often a click.

Maintain a dry delay equal to:

```text
model algorithmic delay
+ resampler delay differences
+ any fixed post-processing delay
```

Signal graph:

```text
canonical input -> DeepFilterNet -> wet -------------------┐
       |                                                    |
       +-> dry delay matching wet path -> dry -------------+-> crossfade -> output
```

Requirements:

- Crossfade over a short configurable interval.
- Use equal-power or another tested fade law.
- Preserve the same output timeline during enable, bypass, and recovery.
- Recalculate delay when the processing graph changes.

## 12. Worker loop

Illustrative pseudocode:

```rust
fn run(mut self, control_rx: Receiver<WorkerCommand>) {
    self.warm_up_model();
    self.signal_ready();

    while !self.cancelled() {
        self.apply_pending_commands(&control_rx);

        let input_frames = self.input_ring.read_into(&mut self.input_scratch);
        if input_frames == 0 {
            self.park_briefly();
            continue;
        }

        self.input_mapper.process(...);
        self.input_resampler.process(...);

        while self.frame_assembler.has_complete_model_frame() {
            let started = Instant::now();

            let model_input = self.frame_assembler.next_frame();
            let result = self.noise_reducer.process(model_input, &mut self.wet_frame);
            self.inference_stats.record(started.elapsed());

            match result {
                Ok(stats) => self.use_wet_output(stats),
                Err(error) => self.handle_model_error(error),
            }

            self.dry_delay.process(...);
            self.crossfader.process(...);
            self.limiter.process(...);
            self.output_resampler.process(...);
            self.output_ring.write_bounded(...);
        }

        self.metrics.publish_snapshot();
    }

    self.fade_and_shutdown();
}
```

The implementation may use parking, a non-blocking notification primitive, or short sleeps when idle. It must not use a busy loop that wastes a CPU core.

## 13. Route lifecycle state

```rust
pub enum PipelineState {
    Stopped,
    Starting,
    Running,
    Recovering,
    Failed,
    Stopping,
}
```

Only the supervisor mutates the authoritative route state. Callbacks and workers send compact observations or faults; they do not independently rebuild routes.

## 14. Metrics

At minimum:

```rust
pub struct PipelineMetrics {
    pub input_peak: f32,
    pub input_rms: f32,
    pub output_peak: f32,
    pub output_rms: f32,

    pub inference_last_us: u64,
    pub inference_p95_us: u64,
    pub inference_p99_us: u64,
    pub missed_deadlines: u64,

    pub input_ring_fill_frames: u32,
    pub output_ring_fill_frames: u32,
    pub input_ring_high_water_frames: u32,
    pub output_ring_low_water_frames: u32,

    pub input_overruns: u64,
    pub output_underruns: u64,
    pub discontinuities: u64,
    pub model_resets: u64,

    pub drift_correction_ppm: i32,
    pub degraded: bool,
    pub overload_events: u64,
    pub skipped_inferences: u64,
    pub estimated_latency_ms: f32,
}
```

Callbacks should update primitive atomics. A separate publisher can aggregate percentiles and create serializable snapshots.

## 15. Initial performance targets

These are engineering targets to validate, not promises:

- Model frame deadline: derived from the loaded model, expected around 10 ms for the low-latency configuration.
- Model inference p99: below 60-70% of the frame deadline on supported hardware.
- Wired end-to-end monitor latency: target approximately 40-70 ms after tuning.
- No sustained queue growth during a two-hour run.
- No callback blocks attributable to engine synchronization.
- Recovery from a single output underrun without route restart.

Bluetooth devices can add substantial transport latency outside the application's control and should be identified clearly in diagnostics.
