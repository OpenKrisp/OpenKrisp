# DeepFilterNet Integration

## 1. Purpose

The `denoise-dfn` crate isolates DeepFilterNet and its inference runtime from the rest of the application. The audio engine must depend on a small internal abstraction rather than upstream implementation details.

This allows the project to:

- Pin and upgrade DeepFilterNet deliberately.
- Keep model-specific array layouts out of the engine.
- Test the engine with a fake noise reducer.
- Replace the inference backend later without changing CPAL or Tauri code.

## 2. Dependency strategy

During implementation:

1. Select a known upstream DeepFilterNet revision.
2. Verify the Rust API and low-latency model feature at that exact revision.
3. Pin the dependency by Git revision or vendor it according to project policy.
4. Record the model asset hash in a manifest.
5. Avoid tracking a moving branch in production.
6. Document any upstream patches maintained by the project.

Do not hard-code frame sizes solely from documentation. Load or derive sample rate, hop size, channel requirements, and latency from the actual model/runtime API when possible.

## 3. Internal interface

```rust
pub trait NoiseReducer {
    fn descriptor(&self) -> &NoiseReducerDescriptor;

    fn process_planar(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<NoiseReductionStats, DenoiseError>;

    fn update_settings(
        &mut self,
        settings: &DenoiseSettings,
    ) -> Result<(), DenoiseError>;

    fn reset(&mut self) -> Result<(), DenoiseError>;
}

pub struct NoiseReducerDescriptor {
    pub model_id: String,
    pub sample_rate_hz: u32,
    pub channels: usize,
    pub frame_samples_per_channel: usize,
    pub algorithmic_latency_samples: usize,
}

pub struct NoiseReductionStats {
    pub attenuation_db: Option<f32>,
    pub speech_probability: Option<f32>,
}
```

Do not require `NoiseReducer: Send + Sync` unless the pinned implementation demonstrably supports it. The concrete processor is confined to one DSP worker.

## 4. Thread confinement

Construct the DeepFilterNet processor inside the DSP thread closure:

```rust
pub fn spawn_worker(init: WorkerInit) -> Result<WorkerHandle, EngineError> {
    let join = std::thread::Builder::new()
        .name(format!("dfn-worker-{}", init.route_id))
        .spawn(move || {
            let reducer = DeepFilterProcessor::load(
                &init.model_asset,
                &init.settings,
            );

            match reducer {
                Ok(reducer) => PipelineWorker::new(init, reducer).run(),
                Err(error) => init.fault_sink.report(error.into()),
            }
        })?;

    Ok(WorkerHandle::new(join))
}
```

Do not create the model in Tauri-managed state and move it later. Keep creation, inference, reset, and destruction on the same worker whenever practical.

## 5. Model asset layout

Recommended structure:

```text
models/
├── manifest.json
└── deepfilternet-low-latency.tar.gz
```

Example manifest shape:

```json
{
  "schemaVersion": 1,
  "models": [
    {
      "id": "deepfilternet-low-latency",
      "file": "deepfilternet-low-latency.tar.gz",
      "sha256": "REPLACE_WITH_PINNED_HASH",
      "upstreamRevision": "REPLACE_WITH_PINNED_REVISION",
      "purpose": "real-time-speech-enhancement"
    }
  ]
}
```

Requirements:

- Resolve paths through Tauri or application resource APIs outside the audio callback.
- Reject missing or hash-mismatched assets.
- Avoid downloading or replacing models silently.
- Store derived model metadata in the route status.
- Ensure packaging includes the same asset validated in development.

## 6. Model input/output layout

The adapter is responsible for translating engine buffers into the exact model layout.

The engine should expose canonical `f32` audio. The adapter should own:

- Interleaved-to-planar conversion, if required.
- Planar-to-interleaved conversion, if required.
- Model-specific array or tensor views.
- Reusable input/output scratch buffers.
- Recurrent state and internal normalization.

All buffers must be allocated during processor initialization.

## 7. Processing contract

For every model call:

- Input length must exactly match `channels * frame_samples_per_channel`.
- Output length must match the same timeline unless the adapter explicitly reports otherwise.
- NaN and infinite input samples must be sanitized or rejected according to policy.
- NaN and infinite model outputs must trigger a safe fallback.
- The adapter must not resize caller-owned buffers.
- The adapter must return an error rather than panic on invalid dimensions.

## 8. Model settings

Expose only settings confirmed by the pinned upstream API. An internal settings type can include:

```rust
pub struct DenoiseSettings {
    pub enabled: bool,
    pub attenuation_limit_db: Option<f32>,
    pub post_filter_enabled: bool,
    pub strength: f32,
}
```

Rules:

- Validate ranges in the control plane and again at the adapter boundary.
- Apply safe runtime changes between model frames.
- If a setting requires model reconstruction, return a typed `RequiresRestart` result.
- Avoid inventing a generic `strength` mapping until it has a documented relationship to actual DeepFilterNet parameters.

## 9. Warm-up

Before the route becomes `Running`:

1. Load and validate the model.
2. Allocate all tensor and conversion buffers.
3. Process several silent frames.
4. Measure warm-up inference time separately from steady state.
5. Reset if warm-up changes state in a way that should not affect the first live frame.
6. Signal readiness to the supervisor.

Warm-up must occur before CPAL input begins filling the live pipeline, or the queue must be sized to tolerate it.

## 10. Reset policy

Reset model state after:

- A route restart.
- A large input discontinuity.
- An overflow that drops a meaningful block of audio.
- A sample-rate or channel-layout change.
- A model error indicating invalid temporal state.
- Resume from a long system sleep.

A tiny output underrun does not automatically require a model reset because the input timeline may still be continuous.

## 11. Error model

```rust
pub enum DenoiseError {
    ModelAssetMissing,
    ModelHashMismatch,
    ModelLoadFailed(String),
    UnsupportedModelFormat,
    InvalidFrameLength { expected: usize, actual: usize },
    InvalidChannelCount { expected: usize, actual: usize },
    InvalidSetting(String),
    NonFiniteInput,
    NonFiniteOutput,
    InferenceFailed(String),
    ResetFailed(String),
    RequiresRestart,
}
```

Do not include raw PCM or sensitive paths in UI-facing errors. Convert internal errors to stable error codes and sanitized descriptions.

## 12. Fallback behavior

When one inference call fails:

1. Do not panic.
2. Produce the latency-aligned dry frame or silence.
3. Increment the failure counter.
4. Attempt a state reset if the error is recoverable.
5. Notify the supervisor after a threshold or immediately for fatal errors.
6. Enter sustained bypass if repeated inference failures would otherwise cause underruns.

The route should remain time-aligned during fallback.

## 13. Output safety

After DeepFilterNet:

- Check for non-finite values.
- Apply a conservative limiter or saturating clamp.
- Measure output peak and RMS.
- Avoid automatic gain compensation until tested; noise reduction can change perceived loudness, but unvalidated gain may create clipping or pumping.

## 14. Testing requirements

### Unit tests

- Reject incorrect frame lengths.
- Reject or sanitize non-finite input according to policy.
- Produce finite output for silence.
- Preserve expected output length.
- Reset returns the processor to a reproducible state where feasible.
- Runtime settings validate correctly.
- Model manifest and hash validation work.

### Integration tests

- Load the packaged model from the same layout used by the application.
- Process deterministic noisy speech fixtures.
- Confirm no panic over thousands of frames.
- Confirm inference timing metrics are collected.
- Confirm one processor is not concurrently accessed from multiple threads.

### Regression tests

Record model revision, fixture revision, and output summary metrics. Avoid requiring bit-identical floating-point output across unrelated architectures unless the runtime guarantees it.
