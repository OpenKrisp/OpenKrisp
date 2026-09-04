use serde::{Deserialize, Serialize};

/// Low-rate snapshot of route health; no PCM is included.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub overload_events: u64,
    #[serde(default)]
    pub skipped_inferences: u64,
    pub estimated_latency_ms: f32,
}
