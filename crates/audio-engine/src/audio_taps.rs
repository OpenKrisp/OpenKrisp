use audio_cpal::{AudioTapControl, AudioTapReader, StreamDirection};
use audio_protocol::RouteId;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct TapSlot {
    route_id: Option<RouteId>,
    capture: Option<AudioTapReader>,
    render: Option<AudioTapReader>,
    capture_control: Option<AudioTapControl>,
    render_control: Option<AudioTapControl>,
}
impl TapSlot {
    fn close(&self) {
        for control in [&self.capture_control, &self.render_control]
            .into_iter()
            .flatten()
        {
            control.close();
        }
    }
}

/// Two route slots, each with one capture and one render SPSC consumer. This registry is used
/// only on control/owner threads; take readers before entering a real-time processing loop.
/// Replacement closes previously taken readers too. No PCM or reader is serializable.
#[derive(Clone, Default)]
pub struct NativeAudioTaps {
    slots: Arc<Mutex<[TapSlot; 2]>>,
}
impl std::fmt::Debug for NativeAudioTaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeAudioTaps { .. }")
    }
}
impl NativeAudioTaps {
    /// Take the current stream generation's sole reader. Poll again after that reader closes
    /// during device recovery. Startup readers remain inactive until the route's gate opens.
    pub fn take(&self, route_id: &RouteId, direction: StreamDirection) -> Option<AudioTapReader> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let slot = slots
            .iter_mut()
            .find(|slot| slot.route_id.as_ref() == Some(route_id))?;
        match direction {
            StreamDirection::Input => slot.capture.take(),
            StreamDirection::Output => slot.render.take(),
        }
    }
    pub(crate) fn register(
        &self,
        route_id: &RouteId,
        incoming: bool,
        direction: StreamDirection,
        reader: AudioTapReader,
    ) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let slot = &mut slots[usize::from(incoming)];
        if slot.route_id.as_ref() != Some(route_id) {
            slot.close();
            *slot = TapSlot {
                route_id: Some(route_id.clone()),
                ..TapSlot::default()
            };
        }
        let (current, control) = match direction {
            StreamDirection::Input => (&mut slot.capture, &mut slot.capture_control),
            StreamDirection::Output => (&mut slot.render, &mut slot.render_control),
        };
        if let Some(control) = control {
            control.close();
        }
        *control = Some(reader.control());
        *current = Some(reader);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_protocol::{AudioFormat, ChannelCount, SampleFormat, SampleRateHz};
    use std::sync::atomic::{AtomicBool, Ordering};
    fn tap(cancelled: Arc<AtomicBool>) -> (audio_cpal::AudioTapWriter, AudioTapReader) {
        audio_cpal::audio_tap(
            AudioFormat {
                sample_rate_hz: SampleRateHz::new(48_000).unwrap(),
                channels: ChannelCount::new(2).unwrap(),
                sample_format: SampleFormat::F32,
                buffer_size_frames: None,
            },
            StreamDirection::Output,
            Arc::new(AtomicBool::new(true)),
            cancelled,
        )
        .unwrap()
    }
    #[test]
    fn taking_and_replacing_generations_preserves_independent_route_ownership() {
        let registry = NativeAudioTaps::default();
        let microphone = RouteId::new("mic").unwrap();
        let incoming = RouteId::new("incoming").unwrap();
        let mic_cancel = Arc::new(AtomicBool::new(false));
        let incoming_cancel = Arc::new(AtomicBool::new(false));
        let (_mic, rx) = tap(mic_cancel.clone());
        registry.register(&microphone, false, StreamDirection::Output, rx);
        let mic_reader = registry.take(&microphone, StreamDirection::Output).unwrap();
        assert!(
            registry
                .take(&microphone, StreamDirection::Output)
                .is_none()
        );
        let (_incoming, rx) = tap(incoming_cancel.clone());
        registry.register(&incoming, true, StreamDirection::Output, rx);
        let incoming_reader = registry.take(&incoming, StreamDirection::Output).unwrap();
        let (_replacement, rx) = tap(incoming_cancel.clone());
        registry.register(&incoming, true, StreamDirection::Output, rx);
        assert!(incoming_reader.is_closed());
        assert!(mic_reader.is_active());
        let new_reader = registry.take(&incoming, StreamDirection::Output).unwrap();
        assert!(new_reader.is_active());
        incoming_cancel.store(true, Ordering::Release);
        assert!(new_reader.is_closed());
        assert!(mic_reader.is_active());
        mic_cancel.store(true, Ordering::Release);
        assert!(mic_reader.is_closed());
        assert_eq!(registry.slots.lock().unwrap().len(), 2);
    }
    #[test]
    fn new_route_identity_closes_both_old_directions_even_after_readers_are_taken() {
        let registry = NativeAudioTaps::default();
        let old = RouteId::new("old").unwrap();
        let new = RouteId::new("new").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let (_capture, c) = tap(cancel.clone());
        let (_render, r) = tap(cancel.clone());
        registry.register(&old, false, StreamDirection::Input, c);
        registry.register(&old, false, StreamDirection::Output, r);
        let c = registry.take(&old, StreamDirection::Input).unwrap();
        let r = registry.take(&old, StreamDirection::Output).unwrap();
        let (_next, next) = tap(cancel);
        registry.register(&new, false, StreamDirection::Input, next);
        assert!(c.is_closed());
        assert!(r.is_closed());
        assert!(registry.take(&old, StreamDirection::Output).is_none());
        assert!(registry.take(&new, StreamDirection::Output).is_none());
        assert!(
            registry
                .take(&new, StreamDirection::Input)
                .unwrap()
                .is_active()
        );
    }
}
