use serde::{Deserialize, Serialize};

use crate::{DeviceId, VirtualEndpointId};

/// Implemented routing features; endpoint readiness is reported separately.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingCapabilities {
    pub physical_input: bool,
    pub physical_output: bool,
    pub system_loopback: bool,
    pub virtual_microphone: bool,
    pub virtual_speaker: bool,
    pub requires_external_driver: bool,
    pub supports_render_reference: bool,
}

/// Shared recursive paths, including aliases. Incomplete ancestry must fail closed for virtual routes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointAncestry {
    pub recursive_paths: Vec<DeviceId>,
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointAvailability {
    Ready,
    Missing,
    Unsupported,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformEndpoint {
    pub id: VirtualEndpointId,
    pub name: String,
    pub availability: EndpointAvailability,
    /// The route owner can create this endpoint when it is missing.
    #[serde(default)]
    pub can_create: bool,
    pub playback_device_id: Option<DeviceId>,
    pub capture_device_id: Option<DeviceId>,
    pub ancestry: EndpointAncestry,
    pub instructions: Vec<String>,
    pub setup_url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingSnapshot {
    pub capabilities: RoutingCapabilities,
    pub endpoints: Vec<PlatformEndpoint>,
}
