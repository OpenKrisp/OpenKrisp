use audio_dsp::{CrossfadeTarget, EqualPowerCrossfade, FixedDryDelay};
use audio_protocol::ChannelCount;

#[test]
fn wet_and_bypass_paths_keep_the_same_timeline() {
    let channels = ChannelCount::new(1).expect("valid channel count");
    let latency_frames = 8;
    let mut dry_delay = FixedDryDelay::new(channels, latency_frames).expect("valid dry delay");
    let mut wet_delay = FixedDryDelay::new(channels, latency_frames).expect("valid wet delay");
    let mut crossfade =
        EqualPowerCrossfade::new(channels, 4, CrossfadeTarget::Wet).expect("valid crossfade");

    let mut input = [0.0; 24];
    input[3] = 1.0;
    input[15] = 0.5;
    let mut dry = [0.0; 24];
    let mut wet = [0.0; 24];
    let mut output = [0.0; 24];
    dry_delay.process(&input, &mut dry).expect("valid buffer");
    wet_delay.process(&input, &mut wet).expect("valid buffer");

    crossfade.set_target(CrossfadeTarget::Dry);
    crossfade
        .process(&dry, &wet, &mut output)
        .expect("aligned buffers");

    assert_eq!(output.len(), input.len());
    let non_zero_indices: Vec<_> = output
        .iter()
        .enumerate()
        .filter_map(|(index, sample)| (sample.abs() > f32::EPSILON).then_some(index))
        .collect();
    assert_eq!(non_zero_indices, [3 + latency_frames, 15 + latency_frames]);
}
