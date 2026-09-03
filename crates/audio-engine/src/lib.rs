#![forbid(unsafe_code)]

//! Audio route orchestration and deterministic backend simulation.
//!
//! Phase 1 provides bounded callback transport and fake endpoints. Physical stream ownership and
//! the production worker/supervisor lifecycle remain Phase 2 work.

mod fake_route;
mod simulation;
mod transport;

pub use fake_route::{SimulatedRoute, SimulationError, SimulationReport};
pub use simulation::{
    CallbackEvent, CallbackSchedule, FakeAudioSink, FakeAudioSource, FakeEndpointError,
    FakeEndpointFault, FakeEndpointStats, ScheduledCallback, SimulatedClock,
};
pub use transport::{
    AudioRingConsumer, AudioRingMetrics, AudioRingMetricsSnapshot, AudioRingProducer,
    AudioTransportError, ReadReport, WriteReport, bounded_audio_ring,
};
