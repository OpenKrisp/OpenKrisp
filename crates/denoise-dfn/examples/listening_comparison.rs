//! Offline companion to the macOS listening check. Input/output are interleaved float32 LE
//! at 48 kHz, with one or two channels. No devices, recording, or system routing are used.
use std::{error::Error, fs, io, path::PathBuf, time::Instant};

use audio_protocol::{ChannelCount, DenoiseSettings};
use denoise_core::{IndependentChannels, NoiseReducer};
use denoise_dfn::{DfnNoiseReducer, ModelAsset, default_model_path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            io::Error::other("Usage: listening_comparison INPUT.f32 OUTPUT.f32 CHANNELS").into(),
        );
    }
    let channels = args[2]
        .to_str()
        .ok_or_else(|| io::Error::other("invalid channels"))?
        .parse::<u16>()?;
    if !(1..=2).contains(&channels) {
        return Err(io::Error::other("mono/stereo only").into());
    }
    let channels = ChannelCount::new(channels)?;
    let input_path = PathBuf::from(&args[0]);
    if fs::metadata(&input_path)?.len() > 120 * 48_000 * u64::from(channels.get()) * 4 {
        return Err(io::Error::other("fixture must not exceed two minutes").into());
    }
    let bytes = fs::read(input_path)?;
    if bytes.is_empty() || !bytes.len().is_multiple_of(channels.as_usize() * 4) {
        return Err(io::Error::other("fixture must contain complete float32 frames").into());
    }
    let samples: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect();
    if samples
        .iter()
        .any(|sample| !sample.is_finite() || sample.abs() > 0.98)
    {
        return Err(io::Error::other("fixture samples must be finite and within ±0.98").into());
    }
    let asset = ModelAsset::load(default_model_path())?;
    let mut reducer = IndependentChannels::new(channels, || {
        let mut reducer = DfnNoiseReducer::from_asset(asset.clone(), DenoiseSettings::default())?;
        reducer.warm_up(3)?;
        Ok(reducer)
    })?;
    let descriptor = reducer.descriptor().clone();
    if descriptor.sample_rate_hz.get() != 48_000 {
        return Err(io::Error::other("fixture rate differs from model").into());
    }
    let hop = descriptor.frame_samples_per_channel;
    let delay = descriptor.algorithmic_latency_samples;
    let frames = samples.len() / channels.as_usize();
    let padded_frames = (frames + delay).div_ceil(hop) * hop;
    let mut input = vec![0.0; hop * channels.as_usize()];
    let mut output = vec![0.0; input.len()];
    let mut rendered = vec![0.0f32; padded_frames * channels.as_usize()];
    let mut timings = Vec::with_capacity(padded_frames / hop);
    for offset in (0..padded_frames).step_by(hop) {
        input.fill(0.0);
        for channel in 0..channels.as_usize() {
            for frame in 0..hop {
                if offset + frame < frames {
                    input[channel * hop + frame] =
                        samples[(offset + frame) * channels.as_usize() + channel];
                }
            }
        }
        let started = Instant::now();
        reducer.process_planar(&input, &mut output)?;
        timings.push(started.elapsed().as_micros());
        for channel in 0..channels.as_usize() {
            for frame in 0..hop {
                let sample = output[channel * hop + frame];
                if !sample.is_finite() {
                    return Err(io::Error::other("non-finite processed output").into());
                }
                rendered[(offset + frame) * channels.as_usize() + channel] =
                    sample.clamp(-0.98, 0.98);
            }
        }
    }
    // Remove known model latency for a same-duration offline A/B comparison. This does not
    // remove latency from a live route or claim to exercise CPAL/resampling/route scheduling.
    let aligned = &rendered[delay * channels.as_usize()..(delay + frames) * channels.as_usize()];
    let bytes: Vec<u8> = aligned
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    std::io::Write::write_all(&mut destination, &bytes)?;
    timings.sort_unstable();
    println!(
        "Processed {frames} stereo/mono frames at 48000 Hz; channels={}; aligned model delay={delay} frames; p95={} us; p99={} us; maximum={} us",
        channels.get(),
        timings[(timings.len() - 1) * 95 / 100],
        timings[(timings.len() - 1) * 99 / 100],
        timings[timings.len() - 1]
    );
    Ok(())
}
