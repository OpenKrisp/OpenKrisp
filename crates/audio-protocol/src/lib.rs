#![forbid(unsafe_code)]

//! Shared, serializable domain types for the OpenKrisp control plane.
//!
//! PCM buffers deliberately do not appear in this crate. The types here are safe to use in
//! engine snapshots and, later, at the Tauri boundary.

mod device;
mod error;
mod event;
mod identifiers;
mod metrics;
mod route;
mod settings;
mod snapshot;

pub use device::{
    AudioDeviceDescriptor, AudioFormat, BufferSizeFrames, ChannelCount, DeviceInventory,
    SampleFormat, SampleRateHz, UnitError,
};
pub use error::{EngineErrorDto, ErrorCode};
pub use event::EngineEvent;
pub use identifiers::{
    ApplicationSinkId, ApplicationStreamId, DeviceId, IdentifierError, LoopbackEndpointId, RouteId,
    VirtualEndpointId,
};
pub use metrics::PipelineMetrics;
pub use route::{DestinationEndpoint, PipelineState, ProcessingMode, RouteConfig, SourceEndpoint};
pub use settings::{DenoiseSettings, SettingsError};
pub use snapshot::{EngineSnapshot, ModelInfo, RouteSnapshot, StreamDiagnostics};
