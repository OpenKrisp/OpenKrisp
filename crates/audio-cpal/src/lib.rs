#![forbid(unsafe_code)]

//! CPAL device discovery, format negotiation, conversion, and callback boundary.

mod devices;
mod error;
mod format;
mod stream;

pub use devices::{CpalHost, DeviceSelection, ResolvedDevice};
pub use error::{
    AudioIoError, AudioIoErrorKind, StreamIssueKind, StreamIssueSnapshot, StreamIssueStore,
};
pub use format::{
    NegotiatedStreamConfig, NegotiationPreferences, StreamDirection, protocol_sample_format,
    select_supported_config,
};
pub use stream::{CallbackMetricsSnapshot, CpalStream};
