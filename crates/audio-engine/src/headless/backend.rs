//! Generation creation is injected only on the route owner; callbacks keep concrete CPAL code.
use super::*;

pub(super) trait GenerationBackend {
    fn build(
        &mut self,
        config: &HeadlessMonitorConfig,
        asset: &ModelAsset,
        control: &MonitorControl,
    ) -> Result<GenerationBuildOutcome, HeadlessMonitorError>;
}
pub(super) struct NativeBackend;
impl GenerationBackend for NativeBackend {
    fn build(
        &mut self,
        config: &HeadlessMonitorConfig,
        asset: &ModelAsset,
        control: &MonitorControl,
    ) -> Result<GenerationBuildOutcome, HeadlessMonitorError> {
        build_generation(config, asset, control)
    }
}

pub(super) trait OwnedStream {
    fn close_audio_tap(&self);
    fn callback_metrics(&self) -> CallbackMetricsSnapshot;
}
impl OwnedStream for CpalStream {
    fn close_audio_tap(&self) {
        CpalStream::close_audio_tap(self);
    }
    fn callback_metrics(&self) -> CallbackMetricsSnapshot {
        CpalStream::callback_metrics(self)
    }
}

pub(super) struct RouteStream(Box<dyn OwnedStream>);
impl RouteStream {
    pub(super) fn new(stream: impl OwnedStream + 'static) -> Self {
        Self(Box::new(stream))
    }
    pub(super) fn close_audio_tap(&self) {
        self.0.close_audio_tap();
    }
    pub(super) fn callback_metrics(&self) -> CallbackMetricsSnapshot {
        self.0.callback_metrics()
    }
}
