use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use audio_protocol::AudioFormat;
use cpal::{FromSample, Sample, SizedSample, traits::DeviceTrait, traits::StreamTrait};

use crate::{
    AudioIoError, AudioTapControl, AudioTapWriter, NegotiatedStreamConfig, ResolvedDevice,
    StreamDirection, StreamIssueStore, error::map_cpal_error,
};

pub(crate) const CALLBACK_CHUNK_FRAMES: usize = 2_048;
const STREAM_BUILD_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
struct CallbackMetricAtoms {
    callbacks: AtomicU64,
    frames: AtomicU64,
}

#[derive(Clone, Debug, Default)]
struct CallbackMetricStore {
    inner: Arc<CallbackMetricAtoms>,
}

impl CallbackMetricStore {
    fn record(&self, frames: usize) {
        self.inner.callbacks.fetch_add(1, Ordering::Relaxed);
        self.inner
            .frames
            .fetch_add(frames as u64, Ordering::Relaxed);
    }

    fn snapshot(&self) -> CallbackMetricsSnapshot {
        CallbackMetricsSnapshot {
            callbacks: self.inner.callbacks.load(Ordering::Relaxed),
            frames: self.inner.frames.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CallbackMetricsSnapshot {
    pub callbacks: u64,
    pub frames: u64,
}

/// Owned CPAL stream with negotiated format and lock-free callback diagnostics.
pub struct CpalStream {
    inner: cpal::Stream,
    direction: StreamDirection,
    format: AudioFormat,
    callbacks: CallbackMetricStore,
    issues: StreamIssueStore,
    tap_control: Option<AudioTapControl>,
}

impl CpalStream {
    /// Invalidate native observations before joining workers or releasing devices.
    pub fn close_audio_tap(&self) {
        if let Some(control) = &self.tap_control {
            control.close();
        }
    }

    pub fn play(&self) -> Result<(), AudioIoError> {
        self.inner
            .play()
            .map_err(|error| map_cpal_error("stream start", error))
    }

    pub fn pause(&self) -> Result<(), AudioIoError> {
        self.inner
            .pause()
            .map_err(|error| map_cpal_error("stream pause", error))
    }

    #[must_use]
    pub const fn direction(&self) -> StreamDirection {
        self.direction
    }

    #[must_use]
    pub const fn format(&self) -> &AudioFormat {
        &self.format
    }

    #[must_use]
    pub fn callback_metrics(&self) -> CallbackMetricsSnapshot {
        self.callbacks.snapshot()
    }

    #[must_use]
    pub fn issue_snapshot(&self) -> crate::StreamIssueSnapshot {
        self.issues.snapshot()
    }
}

impl Drop for CpalStream {
    fn drop(&mut self) {
        self.close_audio_tap();
    }
}

impl ResolvedDevice {
    /// Builds a typed CPAL input callback that converts into bounded normalized chunks.
    pub fn build_input_stream<F>(
        &self,
        config: &NegotiatedStreamConfig,
        on_samples: F,
        issues: StreamIssueStore,
    ) -> Result<CpalStream, AudioIoError>
    where
        F: FnMut(&[f32]) + Send + 'static,
    {
        self.build_input_stream_with_tap(config, on_samples, issues, None)
    }

    /// Builds a stream with an optional bounded native capture/render observation queue.
    pub fn build_input_stream_with_tap<F>(
        &self,
        config: &NegotiatedStreamConfig,
        on_samples: F,
        issues: StreamIssueStore,
        tap: Option<AudioTapWriter>,
    ) -> Result<CpalStream, AudioIoError>
    where
        F: FnMut(&[f32]) + Send + 'static,
    {
        if let Some(tap) = &tap {
            tap.validate_format(config.protocol(), StreamDirection::Input)?;
        }
        let tap_control = tap.as_ref().map(AudioTapWriter::control);
        let callbacks = CallbackMetricStore::default();
        let stream = match config.cpal_sample_format() {
            cpal::SampleFormat::I8 => {
                build_input::<i8, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::I16 => {
                build_input::<i16, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::I24 => {
                build_input::<cpal::I24, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::I32 => {
                build_input::<i32, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::I64 => {
                build_input::<i64, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U8 => {
                build_input::<u8, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U16 => {
                build_input::<u16, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U24 => {
                build_input::<cpal::U24, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U32 => {
                build_input::<u32, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U64 => {
                build_input::<u64, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::F32 => {
                build_input::<f32, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::F64 => {
                build_input::<f64, _>(self, config, on_samples, &callbacks, &issues, tap)
            }
            _ => Err(AudioIoError::NoSupportedFormat { direction: "input" }),
        }?;
        let observed_buffer = stream.buffer_size().ok();
        Ok(CpalStream {
            inner: stream,
            direction: StreamDirection::Input,
            format: config.with_observed_buffer_size(observed_buffer),
            callbacks,
            issues,
            tap_control,
        })
    }

    /// Builds a typed CPAL output callback fed from bounded normalized chunks.
    pub fn build_output_stream<F>(
        &self,
        config: &NegotiatedStreamConfig,
        render_samples: F,
        issues: StreamIssueStore,
    ) -> Result<CpalStream, AudioIoError>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        self.build_output_stream_with_tap(config, render_samples, issues, None)
    }

    /// Builds a stream with an optional bounded native capture/render observation queue.
    pub fn build_output_stream_with_tap<F>(
        &self,
        config: &NegotiatedStreamConfig,
        render_samples: F,
        issues: StreamIssueStore,
        tap: Option<AudioTapWriter>,
    ) -> Result<CpalStream, AudioIoError>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        if let Some(tap) = &tap {
            tap.validate_format(config.protocol(), StreamDirection::Output)?;
        }
        let tap_control = tap.as_ref().map(AudioTapWriter::control);
        let callbacks = CallbackMetricStore::default();
        let stream = match config.cpal_sample_format() {
            cpal::SampleFormat::I8 => {
                build_output::<i8, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::I16 => {
                build_output::<i16, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::I24 => {
                build_output::<cpal::I24, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::I32 => {
                build_output::<i32, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::I64 => {
                build_output::<i64, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U8 => {
                build_output::<u8, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U16 => {
                build_output::<u16, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U24 => {
                build_output::<cpal::U24, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U32 => {
                build_output::<u32, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::U64 => {
                build_output::<u64, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::F32 => {
                build_output::<f32, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            cpal::SampleFormat::F64 => {
                build_output::<f64, _>(self, config, render_samples, &callbacks, &issues, tap)
            }
            _ => Err(AudioIoError::NoSupportedFormat {
                direction: "output",
            }),
        }?;
        let observed_buffer = stream.buffer_size().ok();
        Ok(CpalStream {
            inner: stream,
            direction: StreamDirection::Output,
            format: config.with_observed_buffer_size(observed_buffer),
            callbacks,
            issues,
            tap_control,
        })
    }
}

fn build_input<T, F>(
    device: &ResolvedDevice,
    config: &NegotiatedStreamConfig,
    mut on_samples: F,
    callbacks: &CallbackMetricStore,
    issues: &StreamIssueStore,
    mut tap: Option<AudioTapWriter>,
) -> Result<cpal::Stream, AudioIoError>
where
    T: SizedSample + Copy,
    f32: FromSample<T>,
    F: FnMut(&[f32]) + Send + 'static,
{
    let channels = usize::from(config.stream_config().channels);
    let scratch_samples = CALLBACK_CHUNK_FRAMES
        .checked_mul(channels)
        .ok_or(AudioIoError::CallbackBufferOverflow)?;
    let mut scratch = vec![0.0_f32; scratch_samples];
    let callbacks = callbacks.clone();
    let callback_issues = issues.clone();
    let error_issues = issues.clone();
    let error_tap = tap.as_ref().map(AudioTapWriter::control);
    device
        .inner
        .build_input_stream::<T, _, _>(
            config.stream_config(),
            move |input, info| {
                let aligned_samples = input.len() - input.len() % channels;
                if aligned_samples != input.len() {
                    callback_issues.record_callback_violation();
                    if let Some(tap) = &tap {
                        tap.control().close();
                    }
                }
                callbacks.record(aligned_samples / channels);
                let stamp = tap.as_mut().map(|tap| {
                    let observed_ns = tap.clock().now_ns();
                    let ts = info.timestamp();
                    tap.timestamp(
                        ts.callback.as_nanos(),
                        ts.capture.as_nanos(),
                        observed_ns,
                        aligned_samples / channels,
                    )
                });
                for (index, chunk) in input[..aligned_samples].chunks(scratch_samples).enumerate() {
                    convert_to_f32(chunk, &mut scratch[..chunk.len()]);
                    on_samples(&scratch[..chunk.len()]);
                    if let (Some(tap), Some(stamp)) = (&mut tap, stamp) {
                        tap.write(
                            &scratch[..chunk.len()],
                            stamp.offset(index * CALLBACK_CHUNK_FRAMES),
                        );
                    }
                }
            },
            move |error| {
                if let Some(tap) = &error_tap {
                    if matches!(
                        error.kind(),
                        cpal::ErrorKind::Xrun
                            | cpal::ErrorKind::RealtimeDenied
                            | cpal::ErrorKind::DeviceChanged
                    ) {
                        tap.discontinue();
                    } else {
                        tap.close();
                    }
                }
                error_issues.record_cpal(error);
            },
            Some(STREAM_BUILD_TIMEOUT),
        )
        .map_err(|error| map_cpal_error("input stream construction", error))
}

fn build_output<T, F>(
    device: &ResolvedDevice,
    config: &NegotiatedStreamConfig,
    mut render_samples: F,
    callbacks: &CallbackMetricStore,
    issues: &StreamIssueStore,
    mut tap: Option<AudioTapWriter>,
) -> Result<cpal::Stream, AudioIoError>
where
    T: SizedSample + FromSample<f32>,
    f32: FromSample<T>,
    F: FnMut(&mut [f32]) + Send + 'static,
{
    let channels = usize::from(config.stream_config().channels);
    let scratch_samples = CALLBACK_CHUNK_FRAMES
        .checked_mul(channels)
        .ok_or(AudioIoError::CallbackBufferOverflow)?;
    let mut scratch = vec![0.0_f32; scratch_samples];
    let callbacks = callbacks.clone();
    let callback_issues = issues.clone();
    let error_issues = issues.clone();
    let error_tap = tap.as_ref().map(AudioTapWriter::control);
    device
        .inner
        .build_output_stream::<T, _, _>(
            config.stream_config(),
            move |output, info| {
                let aligned_samples = output.len() - output.len() % channels;
                if aligned_samples != output.len() {
                    callback_issues.record_callback_violation();
                    if let Some(tap) = &tap {
                        tap.control().close();
                    }
                    output[aligned_samples..].fill(T::EQUILIBRIUM);
                }
                callbacks.record(aligned_samples / channels);
                let stamp = tap.as_mut().map(|tap| {
                    let observed_ns = tap.clock().now_ns();
                    let ts = info.timestamp();
                    tap.timestamp(
                        ts.callback.as_nanos(),
                        ts.playback.as_nanos(),
                        observed_ns,
                        aligned_samples / channels,
                    )
                });
                render_output_chunks(
                    &mut output[..aligned_samples],
                    &mut scratch,
                    channels,
                    &mut render_samples,
                    &mut tap,
                    stamp,
                );
            },
            move |error| {
                if let Some(tap) = &error_tap {
                    if matches!(
                        error.kind(),
                        cpal::ErrorKind::Xrun
                            | cpal::ErrorKind::RealtimeDenied
                            | cpal::ErrorKind::DeviceChanged
                    ) {
                        tap.discontinue();
                    } else {
                        tap.close();
                    }
                }
                error_issues.record_cpal(error);
            },
            Some(STREAM_BUILD_TIMEOUT),
        )
        .map_err(|error| map_cpal_error("output stream construction", error))
}

fn render_output_chunks<T, F>(
    output: &mut [T],
    scratch: &mut [f32],
    channels: usize,
    render: &mut F,
    tap: &mut Option<AudioTapWriter>,
    stamp: Option<crate::AudioTimestamp>,
) where
    T: Sample + FromSample<f32>,
    f32: FromSample<T>,
    F: FnMut(&mut [f32]),
{
    let chunk_frames = scratch.len() / channels;
    for (index, chunk) in output.chunks_mut(scratch.len()).enumerate() {
        scratch[..chunk.len()].fill(0.0);
        render(&mut scratch[..chunk.len()]);
        convert_from_f32(&scratch[..chunk.len()], chunk);
        if let (Some(tap), Some(stamp)) = (&mut *tap, stamp) {
            // Observe the samples actually submitted, including integer quantization and silence.
            convert_to_f32(chunk, &mut scratch[..chunk.len()]);
            tap.write(&scratch[..chunk.len()], stamp.offset(index * chunk_frames));
        }
    }
}

fn convert_to_f32<T>(input: &[T], output: &mut [f32])
where
    T: Sample + Copy,
    f32: FromSample<T>,
{
    for (source, destination) in input.iter().copied().zip(output) {
        *destination = f32::from_sample(source);
    }
}

fn convert_from_f32<T>(input: &[f32], output: &mut [T])
where
    T: Sample + FromSample<f32>,
{
    for (source, destination) in input.iter().copied().zip(output) {
        *destination = T::from_sample(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed_output<T>()
    where
        T: Sample + FromSample<f32> + PartialEq + std::fmt::Debug,
        f32: FromSample<T>,
    {
        use audio_protocol::{ChannelCount, SampleFormat, SampleRateHz};
        use std::sync::{Arc, atomic::AtomicBool};
        let format = AudioFormat {
            sample_rate_hz: SampleRateHz::new(44_100).unwrap(),
            channels: ChannelCount::new(2).unwrap(),
            sample_format: SampleFormat::F32,
            buffer_size_frames: None,
        };
        let (writer, mut reader) = crate::audio_tap(
            format,
            StreamDirection::Output,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let mut tap = Some(writer);
        let mut scratch = vec![0.0; CALLBACK_CHUNK_FRAMES * 2];
        let mut position = 0;
        for frames in [17, 64, 256, 480, 512, 2301] {
            let mut output = vec![T::EQUILIBRIUM; frames * 2];
            let observed = tap.as_ref().unwrap().clock().now_ns();
            let stamp = tap.as_mut().unwrap().timestamp(
                80_000_000_000 + position * 1_000_000_000 / 44_100,
                80_010_000_000 + position * 1_000_000_000 / 44_100,
                observed,
                frames,
            );
            let mut available = frames / 2;
            let mut render = |samples: &mut [f32]| {
                for frame in samples.as_chunks_mut::<2>().0 {
                    if available > 0 {
                        frame.copy_from_slice(&[0.312345, -0.234567]);
                        available -= 1;
                    }
                }
            };
            render_output_chunks(
                &mut output,
                &mut scratch,
                2,
                &mut render,
                &mut tap,
                Some(stamp),
            );
            let mut offset = 0;
            while let Some(block) = reader.read() {
                assert_eq!(
                    block.timestamp.frame_position,
                    position as u64 + offset as u64 / 2
                );
                assert_eq!(
                    block.timestamp.estimated_time_ns,
                    stamp
                        .estimated_time_ns
                        .map(|t| t + (offset as i64 / 2 * 1_000_000_000 / 44_100))
                );
                for (index, &reference) in block.samples().iter().enumerate() {
                    assert_eq!(reference, f32::from_sample(output[offset + index]));
                    let frame = (offset + index) / 2;
                    let expected = if frame >= frames / 2 {
                        T::EQUILIBRIUM
                    } else {
                        T::from_sample(if (offset + index) % 2 == 0 {
                            0.312345
                        } else {
                            -0.234567
                        })
                    };
                    assert_eq!(output[offset + index], expected);
                }
                offset += block.samples().len();
            }
            assert_eq!(offset, output.len());
            position += frames as u128;
        }
    }

    #[test]
    fn reference_is_submitted_pcm_including_quantization_stereo_and_underrun_silence() {
        observed_output::<f32>();
        observed_output::<i16>();
        observed_output::<u16>();
    }

    #[test]
    fn observed_output_callback_is_allocation_free() {
        use audio_protocol::{ChannelCount, SampleFormat, SampleRateHz};
        use std::sync::{Arc, atomic::AtomicBool};
        let format = AudioFormat {
            sample_rate_hz: SampleRateHz::new(48_000).unwrap(),
            channels: ChannelCount::new(1).unwrap(),
            sample_format: SampleFormat::I16,
            buffer_size_frames: None,
        };
        let (writer, mut reader) = crate::audio_tap(
            format,
            StreamDirection::Output,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let mut tap = Some(writer);
        let mut output = [0_i16; 512];
        let mut scratch = [0.0; 512];
        let mut render = |s: &mut [f32]| s.fill(0.234567);
        let result = allocation_counter::measure(|| {
            for i in 0..100 {
                let observed = tap.as_ref().unwrap().clock().now_ns();
                let stamp = tap.as_mut().unwrap().timestamp(
                    1_000_000_000 + i * 10_666_667,
                    1_010_000_000 + i * 10_666_667,
                    observed,
                    512,
                );
                render_output_chunks(
                    &mut output,
                    &mut scratch,
                    1,
                    &mut render,
                    &mut tap,
                    Some(stamp),
                );
                for _ in 0..2 {
                    let _ = reader.read();
                }
            }
        });
        assert_eq!(result.count_total, 0, "{result:?}");
    }

    #[test]
    fn signed_and_unsigned_pcm_round_trip_near_normalized_values() {
        let input_i16 = [i16::MIN, -16_384, 0, 16_384, i16::MAX];
        let mut normalized = [0.0; 5];
        convert_to_f32(&input_i16, &mut normalized);
        assert_eq!(normalized[0], -1.0);
        assert!(normalized[4] > 0.999);
        let mut output_i16 = [0_i16; 5];
        convert_from_f32(&normalized, &mut output_i16);
        assert_eq!(output_i16, input_i16);

        let input_u16 = [0_u16, 16_384, 32_768, 49_152, u16::MAX];
        convert_to_f32(&input_u16, &mut normalized);
        assert_eq!(normalized[0], -1.0);
        assert_eq!(normalized[2], 0.0);
        let mut output_u16 = [0_u16; 5];
        convert_from_f32(&normalized, &mut output_u16);
        assert_eq!(output_u16, input_u16);
    }

    #[test]
    fn conversion_helpers_allocate_nothing_in_steady_state() {
        let input = [0_i16; 256];
        let mut normalized = [0.0_f32; 256];
        let mut output = [0_i16; 256];
        let allocations = allocation_counter::measure(|| {
            for _ in 0..100 {
                convert_to_f32(&input, &mut normalized);
                convert_from_f32(&normalized, &mut output);
            }
        });
        assert_eq!(allocations.count_total, 0, "{allocations:?}");
    }
}
