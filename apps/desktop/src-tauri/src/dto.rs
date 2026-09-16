use std::num::NonZeroU16;

use audio_protocol::{
    DenoiseSettings, DestinationEndpoint, DeviceId, LoopbackEndpointId, ProcessingMode,
    RouteConfig, RouteId, SourceEndpoint, VirtualEndpointId,
};
use serde::{Deserialize, Serialize};

use crate::api_error::ApiError;

/// Destination is explicit; omitting its kind preserves the Phase 4 physical-monitor contract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationKind {
    #[default]
    PhysicalOutput,
    VirtualMicrophone,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[default]
    PhysicalInput,
    SystemLoopback,
}

/// Metadata-only request for microphone or incoming loopback routing.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRouteRequest {
    pub route_id: String,
    pub source_device_id: String,
    #[serde(default)]
    pub source_kind: SourceKind,
    pub destination_device_id: String,
    #[serde(default)]
    pub destination_kind: DestinationKind,
    #[serde(default)]
    pub mode: Option<ProcessingMode>,
    pub target_latency_ms: u16,
    pub denoise: DenoiseSettings,
    #[serde(default = "audio_protocol::default_echo_cancellation")]
    pub echo_cancellation: bool,
}

impl TryFrom<StartRouteRequest> for RouteConfig {
    type Error = ApiError;

    fn try_from(request: StartRouteRequest) -> Result<Self, Self::Error> {
        let route_id = RouteId::new(request.route_id)
            .map_err(|_| ApiError::invalid_configuration("Route identifier is invalid."))?;
        let source = match request.source_kind {
            SourceKind::PhysicalInput => {
                SourceEndpoint::PhysicalInput(DeviceId::new(request.source_device_id).map_err(
                    |_| ApiError::invalid_configuration("Input device identifier is invalid."),
                )?)
            }
            SourceKind::SystemLoopback => SourceEndpoint::SystemLoopback(
                LoopbackEndpointId::new(request.source_device_id).map_err(|_| {
                    ApiError::invalid_configuration("Loopback identifier is invalid.")
                })?,
            ),
        };
        let incoming = matches!(request.source_kind, SourceKind::SystemLoopback);
        if incoming && !matches!(request.destination_kind, DestinationKind::PhysicalOutput) {
            return Err(ApiError::invalid_configuration(
                "Incoming audio requires physical headphones or speakers.",
            ));
        }
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
            source,
            destination,
            processing_mode: request.mode.unwrap_or(if incoming {
                ProcessingMode::AlignedBypass
            } else {
                ProcessingMode::SpeechDenoise
            }),
            target_latency_ms,
            denoise: request.denoise,
            echo_cancellation: !incoming && request.echo_cancellation,
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
            source_kind: SourceKind::PhysicalInput,
            destination_device_id: "coreaudio:output".to_owned(),
            destination_kind: DestinationKind::PhysicalOutput,
            mode: Some(ProcessingMode::SpeechDenoise),
            target_latency_ms: 40,
            denoise: DenoiseSettings::default(),
            echo_cancellation: true,
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
    fn echo_cancellation_defaults_on_and_remains_independent_of_noise_bypass() {
        let mut value = serde_json::json!({
            "routeId": "mic", "sourceDeviceId": "mic:physical",
            "destinationDeviceId": "out:physical", "mode": "aligned_bypass",
            "targetLatencyMs": 40, "denoise": {"enabled": false, "attenuationLimitDb": null, "postFilterEnabled": false}
        });
        let parse = |value| {
            RouteConfig::try_from(serde_json::from_value::<StartRouteRequest>(value).unwrap())
                .unwrap()
        };
        let enabled = parse(value.clone());
        assert!(enabled.echo_cancellation);
        assert_eq!(enabled.processing_mode, ProcessingMode::AlignedBypass);
        assert!(!enabled.denoise.enabled);
        value["echoCancellation"] = serde_json::json!(false);
        assert!(!parse(value.clone()).echo_cancellation);
        value["echoCancellation"] = serde_json::json!(true);
        value["sourceKind"] = serde_json::json!("system_loopback");
        assert!(
            !parse(value).echo_cancellation,
            "incoming playback is never echo cancelled"
        );
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
