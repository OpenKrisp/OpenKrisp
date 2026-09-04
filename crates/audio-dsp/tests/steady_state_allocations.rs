use audio_dsp::{
    ChannelLayout, ChannelMapper, CrossfadeTarget, EqualPowerCrossfade, FixedDryDelay, LevelMeter,
    ModelFrameAssembler, SafetyLimiter, StreamingResampler,
};
use audio_protocol::{ChannelCount, SampleRateHz};

#[test]
fn core_dsp_loop_performs_no_steady_state_allocations() {
    let channels = ChannelCount::new(1).expect("valid channel count");
    let mapper =
        ChannelMapper::new(ChannelLayout::Mono, ChannelLayout::Mono).expect("supported mapping");
    let mut assembler = ModelFrameAssembler::new(channels, 480).expect("valid frame size");
    let mut delay = FixedDryDelay::new(channels, 480).expect("valid delay");
    let mut fade =
        EqualPowerCrossfade::new(channels, 32, CrossfadeTarget::Wet).expect("valid crossfade");
    let limiter = SafetyLimiter::new(0.98).expect("valid limiter");
    let mut meter = LevelMeter::new(0.9).expect("valid meter decay");
    let input = [0.25; 480];
    let mut mapped = [0.0; 480];
    let mut dry = [0.0; 480];
    let wet = [0.2; 480];
    let mut output = [0.0; 480];

    let allocations = allocation_counter::measure(|| {
        for _ in 0..100 {
            mapper.process(&input, &mut mapped).expect("valid buffers");
            assembler.push(&mapped, |_| {}).expect("valid frame");
            delay.process(&mapped, &mut dry).expect("valid buffers");
            fade.process(&dry, &wet, &mut output)
                .expect("valid buffers");
            limiter.process_in_place(&mut output);
            meter.process(&output);
        }
    });
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
}

#[test]
fn rubato_adapter_performs_no_steady_state_allocations() {
    let channels = ChannelCount::new(1).expect("valid channel count");
    let input_rate = SampleRateHz::new(44_100).expect("valid rate");
    let output_rate = SampleRateHz::new(48_000).expect("valid rate");
    let mut resampler = StreamingResampler::new(input_rate, output_rate, channels, 441)
        .expect("supported resampler");
    let input = [0.25; 441];
    let mut output = vec![0.0; resampler.output_frames_max()];

    // Resolve any one-time runtime CPU dispatch before the measured section.
    let warmup = resampler
        .process(&input, &mut output)
        .expect("valid buffers");
    assert_eq!(warmup.input_frames_consumed, input.len());
    resampler.reset();

    let allocations = allocation_counter::measure(|| {
        for index in 0_usize..100 {
            let correction_ppm = if index.is_multiple_of(2) { 100 } else { -100 };
            resampler
                .set_correction_ppm(correction_ppm)
                .expect("bounded correction");
            let progress = resampler
                .process(&input, &mut output)
                .expect("valid buffers");
            assert_eq!(progress.input_frames_consumed, input.len());
            assert!(progress.output_frames_written > 0);
        }
    });
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
}
