use audio_protocol::{AudioFormat, BufferSizeFrames, ChannelCount, SampleFormat, SampleRateHz};
use cpal::{BufferSize, SupportedBufferSize, SupportedStreamConfigRange};

use crate::AudioIoError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamDirection {
    Input,
    Output,
}

impl StreamDirection {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

/// Format preferences used by deterministic negotiation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NegotiationPreferences {
    pub sample_rate_hz: SampleRateHz,
    pub channels: ChannelCount,
    pub buffer_size_frames: Option<BufferSizeFrames>,
}

impl NegotiationPreferences {
    #[must_use]
    pub const fn new(sample_rate_hz: SampleRateHz, channels: ChannelCount) -> Self {
        Self {
            sample_rate_hz,
            channels,
            buffer_size_frames: None,
        }
    }

    #[must_use]
    pub const fn with_buffer_size(mut self, frames: BufferSizeFrames) -> Self {
        self.buffer_size_frames = Some(frames);
        self
    }
}

/// Exact CPAL stream request plus its backend-neutral description.
#[derive(Clone, Debug)]
pub struct NegotiatedStreamConfig {
    config: cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    protocol: AudioFormat,
}

impl NegotiatedStreamConfig {
    #[must_use]
    pub const fn stream_config(&self) -> cpal::StreamConfig {
        self.config
    }

    #[must_use]
    pub const fn cpal_sample_format(&self) -> cpal::SampleFormat {
        self.sample_format
    }

    #[must_use]
    pub const fn protocol(&self) -> &AudioFormat {
        &self.protocol
    }

    pub(crate) fn with_observed_buffer_size(&self, frames: Option<u32>) -> AudioFormat {
        let mut format = self.protocol.clone();
        if let Some(observed) = frames.and_then(|value| BufferSizeFrames::new(value).ok()) {
            format.buffer_size_frames = Some(observed);
        }
        format
    }
}

/// Selects the closest supported PCM configuration with deterministic tie-breaking.
pub fn select_supported_config(
    ranges: impl IntoIterator<Item = SupportedStreamConfigRange>,
    direction: StreamDirection,
    preferences: NegotiationPreferences,
) -> Result<NegotiatedStreamConfig, AudioIoError> {
    let preferred_rate = preferences.sample_rate_hz.get();
    let preferred_channels = preferences.channels.get();
    let mut best: Option<((u32, u16, u8), SupportedStreamConfigRange, u32)> = None;

    for range in ranges {
        if !(1..=2).contains(&range.channels())
            || sample_format_rank(range.sample_format()).is_none()
        {
            continue;
        }
        let selected_rate = preferred_rate.clamp(range.min_sample_rate(), range.max_sample_rate());
        let score = (
            selected_rate.abs_diff(preferred_rate),
            range.channels().abs_diff(preferred_channels),
            sample_format_rank(range.sample_format()).unwrap_or(u8::MAX),
        );
        if best.as_ref().is_none_or(|(current, _, _)| score < *current) {
            best = Some((score, range, selected_rate));
        }
    }

    let (_, range, selected_rate) = best.ok_or(AudioIoError::NoSupportedFormat {
        direction: direction.label(),
    })?;
    let buffer_size = requested_buffer_size(range.buffer_size(), preferences.buffer_size_frames);
    let channels =
        ChannelCount::new(range.channels()).map_err(|_| AudioIoError::NoSupportedFormat {
            direction: direction.label(),
        })?;
    let sample_rate_hz =
        SampleRateHz::new(selected_rate).map_err(|_| AudioIoError::NoSupportedFormat {
            direction: direction.label(),
        })?;
    let sample_format =
        protocol_sample_format(range.sample_format()).ok_or(AudioIoError::NoSupportedFormat {
            direction: direction.label(),
        })?;
    let buffer_size_frames = match buffer_size {
        BufferSize::Fixed(frames) => BufferSizeFrames::new(frames).ok(),
        BufferSize::Default => None,
    };

    Ok(NegotiatedStreamConfig {
        config: cpal::StreamConfig {
            channels: range.channels(),
            sample_rate: selected_rate,
            buffer_size,
        },
        sample_format: range.sample_format(),
        protocol: AudioFormat {
            sample_rate_hz,
            channels,
            sample_format,
            buffer_size_frames,
        },
    })
}

#[must_use]
pub fn protocol_sample_format(format: cpal::SampleFormat) -> Option<SampleFormat> {
    Some(match format {
        cpal::SampleFormat::I8 => SampleFormat::I8,
        cpal::SampleFormat::I16 => SampleFormat::I16,
        cpal::SampleFormat::I24 => SampleFormat::I24,
        cpal::SampleFormat::I32 => SampleFormat::I32,
        cpal::SampleFormat::I64 => SampleFormat::I64,
        cpal::SampleFormat::U8 => SampleFormat::U8,
        cpal::SampleFormat::U16 => SampleFormat::U16,
        cpal::SampleFormat::U24 => SampleFormat::U24,
        cpal::SampleFormat::U32 => SampleFormat::U32,
        cpal::SampleFormat::U64 => SampleFormat::U64,
        cpal::SampleFormat::F32 => SampleFormat::F32,
        cpal::SampleFormat::F64 => SampleFormat::F64,
        cpal::SampleFormat::DsdU8 => SampleFormat::DsdU8,
        cpal::SampleFormat::DsdU16 => SampleFormat::DsdU16,
        cpal::SampleFormat::DsdU32 => SampleFormat::DsdU32,
        _ => return None,
    })
}

fn sample_format_rank(format: cpal::SampleFormat) -> Option<u8> {
    Some(match format {
        cpal::SampleFormat::F32 => 0,
        cpal::SampleFormat::F64 => 1,
        cpal::SampleFormat::I16 => 2,
        cpal::SampleFormat::I24 => 3,
        cpal::SampleFormat::I32 => 4,
        cpal::SampleFormat::I8 => 5,
        cpal::SampleFormat::U16 => 6,
        cpal::SampleFormat::U24 => 7,
        cpal::SampleFormat::U32 => 8,
        cpal::SampleFormat::U8 => 9,
        cpal::SampleFormat::I64 => 10,
        cpal::SampleFormat::U64 => 11,
        cpal::SampleFormat::DsdU8 | cpal::SampleFormat::DsdU16 | cpal::SampleFormat::DsdU32 => {
            return None;
        }
        _ => return None,
    })
}

fn requested_buffer_size(
    supported: &SupportedBufferSize,
    requested: Option<BufferSizeFrames>,
) -> BufferSize {
    let Some(requested) = requested else {
        return BufferSize::Default;
    };
    match supported {
        SupportedBufferSize::Range { min, max } if (*min..=*max).contains(&requested.get()) => {
            BufferSize::Fixed(requested.get())
        }
        _ => BufferSize::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(value: u32) -> SampleRateHz {
        SampleRateHz::new(value).expect("valid rate")
    }

    fn channels(value: u16) -> ChannelCount {
        ChannelCount::new(value).expect("valid channels")
    }

    #[test]
    fn negotiation_prefers_canonical_rate_then_layout_then_format() {
        let ranges = [
            SupportedStreamConfigRange::new(
                2,
                44_100,
                44_100,
                SupportedBufferSize::Unknown,
                cpal::SampleFormat::F32,
            ),
            SupportedStreamConfigRange::new(
                1,
                44_100,
                96_000,
                SupportedBufferSize::Range { min: 64, max: 512 },
                cpal::SampleFormat::I16,
            ),
            SupportedStreamConfigRange::new(
                1,
                48_000,
                48_000,
                SupportedBufferSize::Unknown,
                cpal::SampleFormat::F32,
            ),
        ];
        let selected = select_supported_config(
            ranges,
            StreamDirection::Input,
            NegotiationPreferences::new(rate(48_000), channels(1)),
        )
        .expect("format selected");
        assert_eq!(selected.protocol().sample_rate_hz.get(), 48_000);
        assert_eq!(selected.protocol().channels.get(), 1);
        assert_eq!(selected.protocol().sample_format, SampleFormat::F32);
    }

    #[test]
    fn negotiation_rejects_dsd_and_more_than_stereo() {
        let ranges = [
            SupportedStreamConfigRange::new(
                1,
                48_000,
                48_000,
                SupportedBufferSize::Unknown,
                cpal::SampleFormat::DsdU8,
            ),
            SupportedStreamConfigRange::new(
                6,
                48_000,
                48_000,
                SupportedBufferSize::Unknown,
                cpal::SampleFormat::F32,
            ),
        ];
        assert!(matches!(
            select_supported_config(
                ranges,
                StreamDirection::Output,
                NegotiationPreferences::new(rate(48_000), channels(2)),
            ),
            Err(AudioIoError::NoSupportedFormat { .. })
        ));
    }

    #[test]
    fn supported_fixed_buffer_request_is_preserved() {
        let selected = select_supported_config(
            [SupportedStreamConfigRange::new(
                2,
                48_000,
                48_000,
                SupportedBufferSize::Range { min: 64, max: 512 },
                cpal::SampleFormat::F32,
            )],
            StreamDirection::Output,
            NegotiationPreferences::new(rate(48_000), channels(2))
                .with_buffer_size(BufferSizeFrames::new(256).expect("valid buffer size")),
        )
        .expect("format selected");
        assert_eq!(selected.stream_config().buffer_size, BufferSize::Fixed(256));
        assert_eq!(
            selected
                .protocol()
                .buffer_size_frames
                .map(|value| value.get()),
            Some(256)
        );
        assert_eq!(
            selected
                .with_observed_buffer_size(None)
                .buffer_size_frames
                .map(BufferSizeFrames::get),
            Some(256)
        );
    }
}
