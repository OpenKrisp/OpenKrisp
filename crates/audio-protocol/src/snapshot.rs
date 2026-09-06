use serde::{Deserialize, Serialize};

use crate::{
    AudioDeviceDescriptor, AudioFormat, DeviceInventory, EngineErrorDto, PipelineMetrics,
    PipelineState, RouteConfig, SampleRateHz,
};

/// Model shape and fixed latency exposed to diagnostics clients.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub sample_rate_hz: SampleRateHz,
    pub channels: u16,
    pub frame_samples_per_channel: u32,
    pub algorithmic_latency_samples: u32,
}

/// Low-rate counters for one input or output stream.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamDiagnostics {
    pub callbacks: u64,
    pub frames: u64,
    pub issues: u64,
    pub terminal_issues: u64,
    pub xruns: u64,
    pub callback_violations: u64,
    pub last_issue: Option<String>,
    pub last_terminal_issue: Option<String>,
}

/// Reconciliation view of one route. It contains configuration and diagnostics, never PCM.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSnapshot {
    pub config: RouteConfig,
    pub state: PipelineState,
    pub bypassed: bool,
    pub input_device: Option<AudioDeviceDescriptor>,
    pub output_device: Option<AudioDeviceDescriptor>,
    pub input_format: Option<AudioFormat>,
    pub output_format: Option<AudioFormat>,
    pub model: Option<ModelInfo>,
    pub metrics: PipelineMetrics,
    pub input_stream: StreamDiagnostics,
    pub output_stream: StreamDiagnostics,
    pub model_errors: u64,
    pub recovery_attempts: u64,
    pub successful_recoveries: u64,
    pub uptime_seconds: f64,
    pub fault: Option<EngineErrorDto>,
}

impl RouteSnapshot {
    /// Creates a control-plane-only view while route resources are being constructed.
    #[must_use]
    pub fn pending(config: RouteConfig, state: PipelineState) -> Self {
        let bypassed = config.processing_mode == crate::ProcessingMode::AlignedBypass;
        Self {
            config,
            state,
            bypassed,
            input_device: None,
            output_device: None,
            input_format: None,
            output_format: None,
            model: None,
            metrics: PipelineMetrics::default(),
            input_stream: StreamDiagnostics::default(),
            output_stream: StreamDiagnostics::default(),
            model_errors: 0,
            recovery_attempts: 0,
            successful_recoveries: 0,
            uptime_seconds: 0.0,
            fault: None,
        }
    }
}

/// Immutable point-in-time view used to reconcile the desktop UI after reloads.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub revision: u64,
    pub shutting_down: bool,
    pub inventory: DeviceInventory,
    pub routes: Vec<RouteSnapshot>,
    pub last_fault: Option<EngineErrorDto>,
}
