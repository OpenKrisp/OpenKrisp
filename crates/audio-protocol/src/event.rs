use serde::{Deserialize, Serialize};

use crate::{EngineErrorDto, PipelineMetrics, PipelineState, RouteId};

/// Lossy notification referencing an already-published authoritative snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EngineObservation {
    pub revision: u64,
    #[serde(flatten)]
    pub event: EngineEvent,
}

/// Control-plane observation emitted by the engine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EngineEvent {
    StateChanged {
        route_id: RouteId,
        state: PipelineState,
    },
    DevicesChanged,
    Metrics {
        route_id: RouteId,
        metrics: PipelineMetrics,
    },
    Fault {
        route_id: Option<RouteId>,
        error: EngineErrorDto,
        recoverable: bool,
    },
}

#[cfg(test)]
mod tests {
    use crate::{ErrorCode, PipelineMetrics};

    use super::*;

    #[test]
    fn observations_include_a_revision_without_changing_event_topics_or_payload_fields() {
        let observation = EngineObservation {
            revision: 42,
            event: EngineEvent::DevicesChanged,
        };
        let encoded = serde_json::to_value(&observation).expect("observation serializes");
        assert_eq!(encoded["revision"], 42);
        assert_eq!(encoded["type"], "devices_changed");
        assert_eq!(
            serde_json::from_value::<EngineObservation>(encoded).expect("observation decodes"),
            observation
        );
    }

    #[test]
    fn event_round_trip_preserves_typed_fields() {
        let event = EngineEvent::Fault {
            route_id: Some(RouteId::new("monitor").expect("valid test route")),
            error: EngineErrorDto::new(ErrorCode::DeviceDisconnected, "Input was removed")
                .with_suggested_action("Reconnect the input device"),
            recoverable: true,
        };
        let encoded = serde_json::to_string(&event).expect("event serializes");
        let decoded: EngineEvent = serde_json::from_str(&encoded).expect("event deserializes");
        assert_eq!(decoded, event);

        let metrics = EngineEvent::Metrics {
            route_id: RouteId::new("monitor").expect("valid test route"),
            metrics: PipelineMetrics {
                output_peak: 0.75,
                output_ring_fill_frames: 960,
                ..PipelineMetrics::default()
            },
        };
        let encoded = serde_json::to_value(&metrics).expect("metrics event serializes");
        assert_eq!(encoded["type"], "metrics");
        assert_eq!(encoded["routeId"], "monitor");
        assert_eq!(encoded["metrics"]["outputRingFillFrames"], 960);
    }
}
