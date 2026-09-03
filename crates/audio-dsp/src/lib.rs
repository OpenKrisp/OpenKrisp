#![forbid(unsafe_code)]

//! Backend-independent, allocation-bounded DSP primitives.

mod channel_mapper;
mod crossfade;
mod delay_line;
mod error;
mod frame_assembler;
mod interleaved;
mod limiter;
mod meters;
mod resampler;

pub use channel_mapper::{ChannelMapper, ChannelMappingPolicy};
pub use crossfade::{CrossfadeTarget, EqualPowerCrossfade};
pub use delay_line::FixedDryDelay;
pub use error::DspError;
pub use frame_assembler::ModelFrameAssembler;
pub use interleaved::{ChannelLayout, InterleavedBlock, InterleavedBlockMut};
pub use limiter::{LimiterStats, SafetyLimiter};
pub use meters::{LevelMeter, LevelSnapshot};
pub use resampler::{ResampleIo, StreamingResampler};
