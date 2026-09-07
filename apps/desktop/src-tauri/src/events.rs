use std::{sync::mpsc::Receiver, thread};

use audio_protocol::{EngineEvent, EngineObservation};
use tauri::{AppHandle, Emitter, Runtime};

pub const STATE_TOPIC: &str = "audio://state-changed";
pub const DEVICES_TOPIC: &str = "audio://devices-changed";
pub const METRICS_TOPIC: &str = "audio://metrics";
pub const FAULT_TOPIC: &str = "audio://fault";

pub fn spawn_event_bridge<R: Runtime>(
    app: AppHandle<R>,
    receiver: Receiver<EngineObservation>,
) -> Result<thread::JoinHandle<()>, std::io::Error> {
    thread::Builder::new()
        .name("openkrisp-events".to_owned())
        .spawn(move || {
            while let Ok(event) = receiver.recv() {
                let topic = event_topic(&event.event);
                let _ = app.emit(topic, event);
            }
        })
}

fn event_topic(event: &EngineEvent) -> &'static str {
    match event {
        EngineEvent::StateChanged { .. } => STATE_TOPIC,
        EngineEvent::DevicesChanged => DEVICES_TOPIC,
        EngineEvent::Metrics { .. } => METRICS_TOPIC,
        EngineEvent::Fault { .. } => FAULT_TOPIC,
    }
}

#[cfg(test)]
mod tests {
    use audio_protocol::{PipelineState, RouteId};

    use super::*;

    #[test]
    fn every_engine_event_has_a_stable_topic() {
        let route_id = RouteId::new("desktop-monitor").expect("valid route ID");
        assert_eq!(
            event_topic(&EngineEvent::StateChanged {
                route_id: route_id.clone(),
                state: PipelineState::Running,
            }),
            STATE_TOPIC
        );
        assert_eq!(event_topic(&EngineEvent::DevicesChanged), DEVICES_TOPIC);
        assert_eq!(
            event_topic(&EngineEvent::Metrics {
                route_id,
                metrics: audio_protocol::PipelineMetrics::default(),
            }),
            METRICS_TOPIC
        );
    }
}
