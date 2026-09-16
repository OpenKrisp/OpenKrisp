//! Native AEC3 with fixed borrowed planes, confined to its constructing DSP worker.
//! This prototype is not an acoustic acceptance claim. No method belongs in a callback.

use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

pub const SAMPLE_RATE_HZ: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 480;
pub type Frame = [f32; FRAME_SAMPLES];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AecError {
    #[error("AEC render requires one or two channels")]
    Channels,
    #[error("native AEC initialization failed")]
    Initialization,
    #[error("AEC samples must be finite and within [-1, 1]")]
    InvalidSample,
    #[error("native AEC returned error {0}")]
    Processing(i32),
}

unsafe extern "C" {
    fn openkrisp_aec_create(channels: i32) -> *mut c_void;
    fn openkrisp_aec_destroy(state: *mut c_void);
    fn openkrisp_aec_render(state: *mut c_void, left: *const f32, right: *const f32) -> i32;
    fn openkrisp_aec_capture(state: *mut c_void, input: *const f32, output: *mut f32) -> i32;
}

pub struct Aec3 {
    state: NonNull<c_void>,
    render_channels: usize,
    valid: bool,
    // Native calls, configuration and destruction are serialized by worker ownership.
    confined: PhantomData<Rc<()>>,
}

impl Aec3 {
    pub fn new(render_channels: usize) -> Result<Self, AecError> {
        if !(1..=2).contains(&render_channels) {
            return Err(AecError::Channels);
        }
        // SAFETY: validated channel count; bridge returns an owned handle or null, catches exceptions.
        let state = NonNull::new(unsafe { openkrisp_aec_create(render_channels as i32) })
            .ok_or(AecError::Initialization)?;
        Ok(Self {
            state,
            render_channels,
            valid: true,
            confined: PhantomData,
        })
    }

    pub fn analyze_render(&mut self, planes: &[Frame]) -> Result<(), AecError> {
        if !self.valid {
            return Err(AecError::Initialization);
        }
        if planes.len() != self.render_channels {
            return Err(AecError::Channels);
        }
        for plane in planes {
            validate(plane)?;
        }
        let right = if self.render_channels == 2 {
            planes[1].as_ptr()
        } else {
            std::ptr::null()
        };
        // SAFETY: planes are borrowed for the full synchronous call, each has exactly 480 samples.
        status(unsafe { openkrisp_aec_render(self.state.as_ptr(), planes[0].as_ptr(), right) })
    }

    pub fn process_capture(&mut self, input: &Frame, output: &mut Frame) -> Result<(), AecError> {
        output.fill(0.0);
        if !self.valid {
            return Err(AecError::Initialization);
        }
        validate(input)?;
        // SAFETY: owned live handle, disjoint borrowed fixed-size buffers; native call retains neither.
        let result = status(unsafe {
            openkrisp_aec_capture(self.state.as_ptr(), input.as_ptr(), output.as_mut_ptr())
        });
        if result.is_err() || !output.iter().all(|v| v.is_finite()) {
            output.fill(0.0);
            return result.and(Err(AecError::InvalidSample));
        }
        Ok(())
    }

    /// Reconstruct all temporal state on this worker. A failed construction preserves the old
    /// handle for destruction only; further processing is rejected until a successful reset.
    pub fn reset(&mut self) -> Result<(), AecError> {
        self.valid = false;
        let fresh = Self::new(self.render_channels)?;
        *self = fresh;
        Ok(())
    }
}

impl Drop for Aec3 {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner, and !Send/!Sync prevents cross-worker destruction.
        unsafe { openkrisp_aec_destroy(self.state.as_ptr()) }
    }
}

fn validate(samples: &Frame) -> Result<(), AecError> {
    if samples.iter().all(|v| v.is_finite() && v.abs() <= 1.0) {
        Ok(())
    } else {
        Err(AecError::InvalidSample)
    }
}
fn status(code: i32) -> Result<(), AecError> {
    if code == 0 {
        Ok(())
    } else {
        Err(AecError::Processing(code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static_assertions::assert_not_impl_any!(Aec3: Send, Sync);

    #[test]
    fn silence_formats_and_invalid_samples() {
        for channels in [1, 2] {
            let mut aec = Aec3::new(channels).expect("AEC builds");
            let render = [[0.0; FRAME_SAMPLES]; 2];
            let mut output = [1.0; FRAME_SAMPLES];
            for _ in 0..30 {
                aec.analyze_render(&render[..channels]).expect("render");
                aec.process_capture(&render[0], &mut output)
                    .expect("capture");
                assert!(output.iter().all(|v| v.is_finite()));
            }
            assert!(aec.analyze_render(&[]).is_err());
            let mut invalid = [0.0; FRAME_SAMPLES];
            for value in [f32::NAN, f32::INFINITY, -f32::INFINITY, 1.01, -1.01] {
                invalid[17] = value;
                output.fill(0.25);
                assert_eq!(
                    aec.process_capture(&invalid, &mut output),
                    Err(AecError::InvalidSample)
                );
                assert_eq!(
                    output, [0.0; FRAME_SAMPLES],
                    "failed capture must not leave a previous frame exposed"
                );
            }
        }
        assert!(Aec3::new(0).is_err());
        assert!(Aec3::new(3).is_err());
    }
}
