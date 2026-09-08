use std::num::NonZeroU16;

use audio_protocol::{
    DenoiseSettings, DestinationEndpoint, DeviceId, ProcessingMode, RouteConfig, RouteId,
    SourceEndpoint, VirtualEndpointId,
};
use serde::{Deserialize, Serialize};

use crate::api_error::ApiError;

/// Destination is explicit; omitting its kind preserves the Phase 4 physical-monitor contract.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationKind {
    #[default]
    PhysicalOutput,
    VirtualMicrophone,
}

/// UI request for microphone monitoring or a supported virtual microphone.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRouteRequest {
    pub route_id: String,
    pub source_device_id: String,
    pub destination_device_id: String,
    #[serde(default)]
    pub destination_kind: DestinationKind,
    pub mode: ProcessingMode,
    pub target_latency_ms: u16,
    pub denoise: DenoiseSettings,
}

impl TryFrom<StartRouteRequest> for RouteConfig {
    type Error = ApiError;

    fn try_from(request: StartRouteRequest) -> Result<Self, Self::Error> {
        let route_id = RouteId::new(request.route_id)
            .map_err(|_| ApiError::invalid_configuration("Route identifier is invalid."))?;
        let source = DeviceId::new(request.source_device_id)
            .map_err(|_| ApiError::invalid_configuration("Input device identifier is invalid."))?;
        let destination = match request.destination_kind {
            DestinationKind::PhysicalOutput => DestinationEndpoint::PhysicalOutput(
                DeviceId::new(request.destination_device_id).map_err(|_| {
                    ApiError::invalid_configuration("Output device identifier is invalid.")
                })?,
            ),
            DestinationKind::VirtualMicrophone => DestinationEndpoint::VirtualMicrophone(
                VirtualEndpointId::new(request.destination_device_id).map_err(|_| {
                    ApiError::invalid_configuration("Virtual endpoint identifier is invalid.")
                })?,
            ),
        };
        let target_latency_ms = NonZeroU16::new(request.target_latency_ms)
            .ok_or_else(|| ApiError::invalid_configuration("Target latency must be non-zero."))?;
        if target_latency_ms.get() > 1_000 {
            return Err(ApiError::invalid_configuration(
                "Target latency must not exceed 1000 milliseconds.",
            ));
        }
        request
            .denoise
            .validate()
            .map_err(|_| ApiError::invalid_configuration("Denoise settings are invalid."))?;

        Ok(Self {
            id: route_id,
            source: SourceEndpoint::PhysicalInput(source),
            destination,
            processing_mode: request.mode,
            target_latency_ms,
            denoise: request.denoise,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateRouteSettingsRequest {
    pub route_id: String,
    pub denoise: DenoiseSettings,
}

impl UpdateRouteSettingsRequest {
    pub fn validate(self) -> Result<(RouteId, DenoiseSettings), ApiError> {
        let route_id = RouteId::new(self.route_id)
            .map_err(|_| ApiError::invalid_configuration("Route identifier is invalid."))?;
        self.denoise
            .validate()
            .map_err(|_| ApiError::invalid_configuration("Denoise settings are invalid."))?;
        Ok((route_id, self.denoise))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetRouteBypassRequest {
    pub route_id: String,
    pub bypassed: bool,
}

impl SetRouteBypassRequest {
    pub fn validate(self) -> Result<(RouteId, bool), ApiError> {
        let route_id = RouteId::new(self.route_id)
            .map_err(|_| ApiError::invalid_configuration("Route identifier is invalid."))?;
        Ok((route_id, self.bypassed))
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteAcceptedDto {
    pub route_id: RouteId,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> StartRouteRequest {
        StartRouteRequest {
            route_id: "desktop-monitor".to_owned(),
            source_device_id: "coreaudio:input".to_owned(),
            destination_device_id: "coreaudio:output".to_owned(),
            destination_kind: DestinationKind::PhysicalOutput,
            mode: ProcessingMode::SpeechDenoise,
            target_latency_ms: 40,
            denoise: DenoiseSettings::default(),
        }
    }

    #[test]
    fn start_request_builds_only_a_physical_route() {
        let config = RouteConfig::try_from(request()).expect("request is valid");
        assert!(matches!(config.source, SourceEndpoint::PhysicalInput(_)));
        assert!(matches!(
            config.destination,
            DestinationEndpoint::PhysicalOutput(_)
        ));
        assert_eq!(config.target_latency_ms.get(), 40);
    }

    #[test]
    fn invalid_values_are_rejected_before_the_engine_queue() {
        let mut invalid = request();
        invalid.target_latency_ms = 0;
        assert!(RouteConfig::try_from(invalid).is_err());

        let mut invalid = request();
        invalid.route_id = "\n".to_owned();
        assert!(RouteConfig::try_from(invalid).is_err());

        let mut invalid = request();
        invalid.denoise.attenuation_limit_db = Some(f32::NAN);
        assert!(RouteConfig::try_from(invalid).is_err());
    }
}
