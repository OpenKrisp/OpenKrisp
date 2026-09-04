#![forbid(unsafe_code)]

//! Audio route orchestration, physical headless monitoring, and deterministic backend simulation.

mod fake_route;
mod headless;
mod metrics;
mod recovery;
mod simulation;
mod state_machine;
mod transport;
mod worker;

pub use fake_route::{SimulatedRoute, SimulationError, SimulationReport};
pub use headless::{
    HeadlessMonitor, HeadlessMonitorConfig, HeadlessMonitorError, HeadlessMonitorSnapshot,
};
pub use metrics::RouteMetricsStore;
pub use recovery::{
    RecoveryAction, RecoveryAttempt, RecoveryBudget, RouteRecoveryPolicy, audio_error_action,
    stream_issue_action,
};
pub use simulation::{
    CallbackEvent, CallbackSchedule, FakeAudioSink, FakeAudioSource, FakeEndpointError,
    FakeEndpointFault, FakeEndpointStats, ScheduledCallback, SimulatedClock,
};
pub use state_machine::{RouteStateMachine, StateTransitionError};
pub use transport::{
    AudioRingConsumer, AudioRingMetrics, AudioRingMetricsSnapshot, AudioRingProducer,
    AudioTransportError, ReadReport, WriteReport, bounded_audio_ring,
};
pub use worker::{
    PipelineWorkerConfig, PipelineWorkerHandle, PipelineWorkerWake, WorkerCommand,
    WorkerCommandError, WorkerError, WorkerStartError, spawn_pipeline_worker,
};
