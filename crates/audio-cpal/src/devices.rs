use std::str::FromStr;

use audio_protocol::{AudioDeviceDescriptor, AudioFormat, DeviceId, DeviceInventory};
use cpal::traits::{DeviceTrait, HostTrait};

use crate::{
    AudioIoError, NegotiatedStreamConfig, NegotiationPreferences, StreamDirection,
    error::map_cpal_error, protocol_sample_format, select_supported_config,
};

/// Selects either the current host default or one stable CPAL device identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceSelection {
    Default,
    Id(DeviceId),
}

/// Default CPAL host used for physical endpoint discovery and resolution.
pub struct CpalHost {
    inner: cpal::Host,
    name: String,
}

impl CpalHost {
    #[must_use]
    pub fn new_default() -> Self {
        let inner = cpal::default_host();
        let name = inner.id().to_string();
        Self { inner, name }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Enumerates stable IDs, default roles, capabilities, and default formats.
    pub fn inventory(&self) -> Result<DeviceInventory, AudioIoError> {
        let default_input_id = self
            .inner
            .default_input_device()
            .and_then(|device| device.id().ok());
        let default_output_id = self
            .inner
            .default_output_device()
            .and_then(|device| device.id().ok());
        let devices = self
            .inner
            .devices()
            .map_err(|error| map_cpal_error("device enumeration", error))?
            .map(|device| {
                describe_device(
                    &device,
                    &self.name,
                    default_input_id.as_ref(),
                    default_output_id.as_ref(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(DeviceInventory {
            host: self.name.clone(),
            devices,
        })
    }

    pub fn resolve_input(
        &self,
        selection: &DeviceSelection,
    ) -> Result<ResolvedDevice, AudioIoError> {
        self.resolve(selection, StreamDirection::Input)
    }

    pub fn resolve_output(
        &self,
        selection: &DeviceSelection,
    ) -> Result<ResolvedDevice, AudioIoError> {
        self.resolve(selection, StreamDirection::Output)
    }

    fn resolve(
        &self,
        selection: &DeviceSelection,
        direction: StreamDirection,
    ) -> Result<ResolvedDevice, AudioIoError> {
        let device = match selection {
            DeviceSelection::Default => match direction {
                StreamDirection::Input => self.inner.default_input_device(),
                StreamDirection::Output => self.inner.default_output_device(),
            }
            .ok_or(AudioIoError::NoDefaultDevice {
                direction: direction.label(),
            })?,
            DeviceSelection::Id(id) => {
                let cpal_id = cpal::DeviceId::from_str(id.as_str())
                    .map_err(|_| AudioIoError::InvalidDeviceId(id.clone()))?;
                self.inner
                    .device_by_id(&cpal_id)
                    .ok_or_else(|| AudioIoError::DeviceNotFound(id.clone()))?
            }
        };
        let default_input_id = self
            .inner
            .default_input_device()
            .and_then(|candidate| candidate.id().ok());
        let default_output_id = self
            .inner
            .default_output_device()
            .and_then(|candidate| candidate.id().ok());
        let descriptor = describe_device(
            &device,
            &self.name,
            default_input_id.as_ref(),
            default_output_id.as_ref(),
        )?;
        Ok(ResolvedDevice {
            inner: device,
            descriptor,
        })
    }
}

impl Default for CpalHost {
    fn default() -> Self {
        Self::new_default()
    }
}

/// Resolved physical device retained only inside the CPAL adapter boundary.
pub struct ResolvedDevice {
    pub(crate) inner: cpal::Device,
    descriptor: AudioDeviceDescriptor,
}

impl ResolvedDevice {
    #[must_use]
    pub const fn descriptor(&self) -> &AudioDeviceDescriptor {
        &self.descriptor
    }

    pub fn negotiate_input(
        &self,
        preferences: NegotiationPreferences,
    ) -> Result<NegotiatedStreamConfig, AudioIoError> {
        let configs = self
            .inner
            .supported_input_configs()
            .map_err(|error| map_cpal_error("input format query", error))?;
        select_supported_config(configs, StreamDirection::Input, preferences)
    }

    pub fn negotiate_output(
        &self,
        preferences: NegotiationPreferences,
    ) -> Result<NegotiatedStreamConfig, AudioIoError> {
        let configs = self
            .inner
            .supported_output_configs()
            .map_err(|error| map_cpal_error("output format query", error))?;
        select_supported_config(configs, StreamDirection::Output, preferences)
    }
}

fn describe_device(
    device: &cpal::Device,
    host: &str,
    default_input_id: Option<&cpal::DeviceId>,
    default_output_id: Option<&cpal::DeviceId>,
) -> Result<AudioDeviceDescriptor, AudioIoError> {
    let cpal_id = device
        .id()
        .map_err(|error| map_cpal_error("device identifier", error))?;
    let id = DeviceId::new(cpal_id.to_string()).map_err(|_| AudioIoError::Backend {
        operation: "device identifier validation",
        kind: crate::AudioIoErrorKind::InvalidInput,
        message: "backend returned an invalid stable device identifier".to_owned(),
    })?;
    let description = device
        .description()
        .map_err(|error| map_cpal_error("device description", error))?;
    let default_input_format = device.default_input_config().ok().and_then(protocol_format);
    let default_output_format = device
        .default_output_config()
        .ok()
        .and_then(protocol_format);
    let supports_input = default_input_format.is_some() || device.supports_input();
    let supports_output = default_output_format.is_some() || device.supports_output();
    Ok(AudioDeviceDescriptor {
        id,
        name: description.name().to_owned(),
        host: host.to_owned(),
        manufacturer: description.manufacturer().map(str::to_owned),
        supports_input,
        supports_output,
        is_default_input: default_input_id == Some(&cpal_id),
        is_default_output: default_output_id == Some(&cpal_id),
        default_input_format,
        default_output_format,
    })
}

fn protocol_format(config: cpal::SupportedStreamConfig) -> Option<AudioFormat> {
    Some(AudioFormat {
        sample_rate_hz: audio_protocol::SampleRateHz::new(config.sample_rate()).ok()?,
        channels: audio_protocol::ChannelCount::new(config.channels()).ok()?,
        sample_format: protocol_sample_format(config.sample_format())?,
        buffer_size_frames: None,
    })
}
