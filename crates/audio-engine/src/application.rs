//! Native application PCM stays in a bounded Rust ring and never enters the control/UI bridge.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use audio_protocol::{AudioFormat, SampleFormat};

use crate::{
    AudioRingConsumer, AudioRingProducer, AudioTransportError, WriteReport, bounded_audio_ring,
};

/// Single producer for a continuous application stream. Pace writes at `format.sample_rate_hz`.
/// Overflow rejects newest complete frames; inspect the report and never retry stale audio forever.
pub struct ApplicationPcmWriter {
    producer: AudioRingProducer,
    connected: Arc<AtomicBool>,
    accepting: Arc<AtomicBool>,
}

/// Move-only source passed directly to the native engine. Contains no serializable PCM API.
pub struct ApplicationPcmSource {
    pub(crate) consumer: AudioRingConsumer,
    pub(crate) format: AudioFormat,
    pub(crate) connected: Arc<AtomicBool>,
    pub(crate) accepting: Arc<AtomicBool>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplicationPcmError {
    #[error("Application PCM requires float32 mono/stereo at 8–192 kHz and 1–1000 ms capacity")]
    InvalidFormat,
    #[error("The application route is not accepting audio")]
    NotRunning,
    #[error(transparent)]
    Transport(#[from] AudioTransportError),
}

pub fn application_pcm_stream(
    format: AudioFormat,
    capacity_ms: u16,
) -> Result<(ApplicationPcmWriter, ApplicationPcmSource), ApplicationPcmError> {
    if format.channels.get() > 2
        || format.sample_format != SampleFormat::F32
        || !(8_000..=192_000).contains(&format.sample_rate_hz.get())
        || !(1..=1000).contains(&capacity_ms)
    {
        return Err(ApplicationPcmError::InvalidFormat);
    }
    let frames =
        (u64::from(format.sample_rate_hz.get()) * u64::from(capacity_ms) / 1000).max(1) as usize;
    let (producer, consumer) = bounded_audio_ring(frames, format.channels)?;
    let connected = Arc::new(AtomicBool::new(true));
    let accepting = Arc::new(AtomicBool::new(false));
    Ok((
        ApplicationPcmWriter {
            producer,
            connected: connected.clone(),
            accepting: accepting.clone(),
        },
        ApplicationPcmSource {
            consumer,
            format,
            connected,
            accepting,
        },
    ))
}

impl ApplicationPcmWriter {
    pub fn is_ready(&self) -> bool {
        self.accepting.load(Ordering::Acquire)
    }

    /// Allocation-free, non-blocking transfer of complete interleaved frames.
    pub fn write(&mut self, samples: &[f32]) -> Result<WriteReport, ApplicationPcmError> {
        if !self.is_ready() {
            return Err(ApplicationPcmError::NotRunning);
        }
        Ok(self.producer.write(samples)?)
    }
}

impl Drop for ApplicationPcmWriter {
    fn drop(&mut self) {
        self.connected.store(false, Ordering::Release);
    }
}

impl std::fmt::Debug for ApplicationPcmSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApplicationPcmSource")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_protocol::{ChannelCount, SampleRateHz};

    #[test]
    fn application_pcm_is_bounded_frame_aligned_and_gated_by_route_lifetime() {
        let format = AudioFormat {
            sample_rate_hz: SampleRateHz::new(48_000).expect("rate"),
            channels: ChannelCount::new(2).expect("stereo"),
            sample_format: SampleFormat::F32,
            buffer_size_frames: None,
        };
        let (mut writer, mut source) = application_pcm_stream(format.clone(), 1).expect("source");
        assert!(matches!(
            writer.write(&[0.1, 0.2]),
            Err(ApplicationPcmError::NotRunning)
        ));
        source.accepting.store(true, Ordering::Release);
        assert!(writer.write(&[0.1]).is_err());
        let signal: Vec<f32> = (0..60)
            .flat_map(|n| [n as f32 / 100., -n as f32 / 100.])
            .collect();
        let written = writer.write(&signal).expect("bounded write");
        assert_eq!(written.written_frames, 48);
        let mut output = [0.0; 120];
        let read = source.consumer.read(&mut output).expect("read");
        assert_eq!(read.read_frames, 48);
        assert_eq!(&output[..96], &signal[..96]);
        source.accepting.store(false, Ordering::Release);
        assert!(matches!(
            writer.write(&signal),
            Err(ApplicationPcmError::NotRunning)
        ));
        drop(writer);
        assert!(!source.connected.load(Ordering::Acquire));
        let mut surround = format;
        surround.channels = ChannelCount::new(6).expect("surround");
        assert!(matches!(
            application_pcm_stream(surround, 100),
            Err(ApplicationPcmError::InvalidFormat)
        ));
    }
}
