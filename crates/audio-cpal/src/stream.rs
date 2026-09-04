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
    AudioIoError, NegotiatedStreamConfig, ResolvedDevice, StreamDirection, StreamIssueStore,
    error::map_cpal_error,
};

const CALLBACK_CHUNK_FRAMES: usize = 2_048;
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
}

impl CpalStream {
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
        let callbacks = CallbackMetricStore::default();
        let stream = match config.cpal_sample_format() {
            cpal::SampleFormat::I8 => {
                build_input::<i8, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::I16 => {
                build_input::<i16, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::I24 => {
                build_input::<cpal::I24, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::I32 => {
                build_input::<i32, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::I64 => {
                build_input::<i64, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U8 => {
                build_input::<u8, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U16 => {
                build_input::<u16, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U24 => {
                build_input::<cpal::U24, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U32 => {
                build_input::<u32, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U64 => {
                build_input::<u64, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::F32 => {
                build_input::<f32, _>(self, config, on_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::F64 => {
                build_input::<f64, _>(self, config, on_samples, &callbacks, &issues)
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
        let callbacks = CallbackMetricStore::default();
        let stream = match config.cpal_sample_format() {
            cpal::SampleFormat::I8 => {
                build_output::<i8, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::I16 => {
                build_output::<i16, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::I24 => {
                build_output::<cpal::I24, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::I32 => {
                build_output::<i32, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::I64 => {
                build_output::<i64, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U8 => {
                build_output::<u8, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U16 => {
                build_output::<u16, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U24 => {
                build_output::<cpal::U24, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U32 => {
                build_output::<u32, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::U64 => {
                build_output::<u64, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::F32 => {
                build_output::<f32, _>(self, config, render_samples, &callbacks, &issues)
            }
            cpal::SampleFormat::F64 => {
                build_output::<f64, _>(self, config, render_samples, &callbacks, &issues)
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
        })
    }
}

fn build_input<T, F>(
    device: &ResolvedDevice,
    config: &NegotiatedStreamConfig,
    mut on_samples: F,
    callbacks: &CallbackMetricStore,
    issues: &StreamIssueStore,
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
    device
        .inner
        .build_input_stream::<T, _, _>(
            config.stream_config(),
            move |input, _| {
                let aligned_samples = input.len() - input.len() % channels;
                if aligned_samples != input.len() {
                    callback_issues.record_callback_violation();
                }
                callbacks.record(aligned_samples / channels);
                for chunk in input[..aligned_samples].chunks(scratch_samples) {
                    convert_to_f32(chunk, &mut scratch[..chunk.len()]);
                    on_samples(&scratch[..chunk.len()]);
                }
            },
            move |error| error_issues.record_cpal(error),
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
) -> Result<cpal::Stream, AudioIoError>
where
    T: SizedSample + FromSample<f32>,
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
    device
        .inner
        .build_output_stream::<T, _, _>(
            config.stream_config(),
            move |output, _| {
                let aligned_samples = output.len() - output.len() % channels;
                if aligned_samples != output.len() {
                    callback_issues.record_callback_violation();
                    output[aligned_samples..].fill(T::EQUILIBRIUM);
                }
                callbacks.record(aligned_samples / channels);
                for chunk in output[..aligned_samples].chunks_mut(scratch_samples) {
                    scratch[..chunk.len()].fill(0.0);
                    render_samples(&mut scratch[..chunk.len()]);
                    convert_from_f32(&scratch[..chunk.len()], chunk);
                }
            },
            move |error| error_issues.record_cpal(error),
            Some(STREAM_BUILD_TIMEOUT),
        )
        .map_err(|error| map_cpal_error("output stream construction", error))
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
