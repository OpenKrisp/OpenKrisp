use audio_protocol::{ChannelCount, DenoiseSettings};

use crate::{DenoiseError, NoiseReducer, NoiseReducerDescriptor, NoiseReductionStats};

/// Independent mono histories in a planar mono/stereo adapter. Construct on the DSP worker.
pub struct IndependentChannels<R> {
    reducers: Vec<R>,
    descriptor: NoiseReducerDescriptor,
}

impl<R: NoiseReducer> IndependentChannels<R> {
    pub fn new<F>(channels: ChannelCount, mut factory: F) -> Result<Self, DenoiseError>
    where
        F: FnMut() -> Result<R, DenoiseError>,
    {
        if channels.get() > 2 {
            return Err(DenoiseError::UnsupportedModelFormat);
        }
        let first = factory()?;
        let mut descriptor = first.descriptor().clone();
        descriptor.frame_samples()?;
        if descriptor.channels.get() != 1 {
            return Err(DenoiseError::InvalidDescriptor);
        }
        let mut reducers = Vec::with_capacity(channels.as_usize());
        reducers.push(first);
        for _ in 1..channels.get() {
            let reducer = factory()?;
            if reducer.descriptor() != &descriptor {
                return Err(DenoiseError::InvalidDescriptor);
            }
            reducers.push(reducer);
        }
        descriptor.channels = channels;
        Ok(Self {
            reducers,
            descriptor,
        })
    }
}

impl<R: NoiseReducer> NoiseReducer for IndependentChannels<R> {
    fn descriptor(&self) -> &NoiseReducerDescriptor {
        &self.descriptor
    }

    fn process_planar(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<NoiseReductionStats, DenoiseError> {
        let expected = self.descriptor.frame_samples()?;
        for actual in [input.len(), output.len()] {
            if actual != expected {
                return Err(DenoiseError::InvalidFrameLength { expected, actual });
            }
        }
        let frames = self.descriptor.frame_samples_per_channel;
        for ((reducer, input), output) in self
            .reducers
            .iter_mut()
            .zip(input.chunks_exact(frames))
            .zip(output.chunks_exact_mut(frames))
        {
            reducer.process_planar(input, output)?;
        }
        Ok(NoiseReductionStats::default())
    }

    fn update_settings(&mut self, settings: &DenoiseSettings) -> Result<(), DenoiseError> {
        for reducer in &mut self.reducers {
            reducer.update_settings(settings)?;
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<(), DenoiseError> {
        // Reset every channel even if an earlier channel fails.
        let mut result = Ok(());
        for reducer in &mut self.reducers {
            if let Err(error) = reducer.reset() {
                result = Err(error);
            }
        }
        result
    }
}
