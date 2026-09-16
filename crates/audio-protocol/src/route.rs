use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};

use crate::{
    ApplicationSinkId, ApplicationStreamId, DenoiseSettings, DeviceId, LoopbackEndpointId, RouteId,
    VirtualEndpointId,
};

/// A source that can provide PCM to a route.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceEndpoint {
    PhysicalInput(DeviceId),
    SystemLoopback(LoopbackEndpointId),
    ApplicationStream(ApplicationStreamId),
    VirtualRouteInput(VirtualEndpointId),
}

/// A destination that can consume route PCM.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DestinationEndpoint {
    PhysicalOutput(DeviceId),
    VirtualMicrophone(VirtualEndpointId),
    ApplicationSink(ApplicationSinkId),
}

/// Whether speech denoising or its latency-aligned dry path is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingMode {
    SpeechDenoise,
    AlignedBypass,
}

/// Authoritative lifecycle state for one route.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineState {
    #[default]
    Stopped,
    Starting,
    Running,
    Reconfiguring,
    Recovering,
    Failed,
    Stopping,
}

/// Immutable configuration for one route startup attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteConfig {
    pub id: RouteId,
    pub source: SourceEndpoint,
    pub destination: DestinationEndpoint,
    pub processing_mode: ProcessingMode,
    pub target_latency_ms: NonZeroU16,
    pub denoise: DenoiseSettings,
    /// Microphone AEC preference, independent of denoise bypass. Ignored on incoming routes.
    #[serde(default = "default_echo_cancellation")]
    pub echo_cancellation: bool,
}

/// New desktop routes and saved profiles without an explicit choice enable microphone AEC.
#[must_use]
pub const fn default_echo_cancellation() -> bool {
    true
}

impl RouteConfig {
    #[must_use]
    pub fn is_incoming(&self) -> bool {
        matches!(
            self.source,
            SourceEndpoint::SystemLoopback(_) | SourceEndpoint::ApplicationStream(_)
        )
    }

    /// Native application streams start in aligned bypass; speech denoising is an explicit action.
    #[must_use]
    pub fn application_stream(
        id: RouteId,
        stream: ApplicationStreamId,
        output: DeviceId,
        target_latency_ms: NonZeroU16,
    ) -> Self {
        Self {
            id,
            source: SourceEndpoint::ApplicationStream(stream),
            destination: DestinationEndpoint::PhysicalOutput(output),
            processing_mode: ProcessingMode::AlignedBypass,
            target_latency_ms,
            denoise: DenoiseSettings::default(),
            echo_cancellation: false,
        }
    }
    /// Validates settings that cannot be encoded directly in the field types.
    pub fn validate(&self) -> Result<(), crate::SettingsError> {
        self.denoise.validate()
    }
}
