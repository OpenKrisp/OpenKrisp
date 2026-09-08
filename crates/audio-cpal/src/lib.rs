#![deny(unsafe_code)]

//! CPAL device discovery, format negotiation, conversion, and callback boundary.

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("OpenKrisp audio I/O supports only macOS and Linux.");

mod devices;
mod error;
mod format;
mod stream;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos_metadata;

#[cfg(target_os = "linux")]
mod linux_metadata;

pub use devices::{CpalHost, DeviceSelection, ResolvedDevice};
pub use error::{
    AudioIoError, AudioIoErrorKind, StreamIssueKind, StreamIssueSnapshot, StreamIssueStore,
};
pub use format::{
    NegotiatedStreamConfig, NegotiationPreferences, StreamDirection, protocol_sample_format,
    select_supported_config,
};
pub use stream::{CallbackMetricsSnapshot, CpalStream};
