#include "webrtc/api/audio/audio_processing.h"
#include "webrtc/modules/audio_processing/aec3/echo_canceller3.h"
#include "webrtc/rtc_base/logging.h"
#include "openkrisp_aec_config.h"
#include <new>

namespace {
class ExplicitChannelsEchoFactory final : public webrtc::EchoControlFactory {
 public:
  std::unique_ptr<webrtc::EchoControl> Create(int sample_rate_hz,
                                           int render_channels,
                                           int capture_channels) override {
    webrtc::EchoCanceller3Config config;
    auto stereo = webrtc::EchoCanceller3::CreateDefaultMultichannelConfig();
    openkrisp::ConfigureAec3(config, stereo);
    return std::make_unique<webrtc::EchoCanceller3>(
        config, stereo, sample_rate_hz, render_channels, capture_channels);
  }
};
}  // namespace

struct OpenKrispAec {
  rtc::scoped_refptr<webrtc::AudioProcessing> processor;
  webrtc::StreamConfig capture{48000, 1};
  webrtc::StreamConfig render;
  explicit OpenKrispAec(int channels) : render(48000, channels) {}
};

extern "C" {
void* openkrisp_aec_create(int channels) noexcept {
  if (channels != 1 && channels != 2) return nullptr;
  try {
    auto state = std::make_unique<OpenKrispAec>(channels);
    rtc::LogMessage::LogToDebug(rtc::LS_NONE);
    rtc::LogMessage::SetLogToStderr(false);
    state->processor = webrtc::AudioProcessingBuilder()
                           .SetEchoControlFactory(
                               std::make_unique<ExplicitChannelsEchoFactory>())
                           .Create();
    if (!state->processor) return nullptr;
    webrtc::AudioProcessing::Config config;
    config.pipeline.multi_channel_render = true;
    config.pipeline.multi_channel_capture = false;
    config.echo_canceller.enabled = true;
    config.echo_canceller.mobile_mode = false;
    config.echo_canceller.enforce_high_pass_filtering = false;
    config.high_pass_filter.enabled = true;
    config.noise_suppression.enabled = false;
    config.gain_controller1.enabled = false;
    config.gain_controller2.enabled = false;
    config.pre_amplifier.enabled = false;
    config.capture_level_adjustment.enabled = false;
    state->processor->ApplyConfig(config);
    webrtc::ProcessingConfig formats;
    formats.input_stream() = state->capture;
    formats.output_stream() = state->capture;
    formats.reverse_input_stream() = state->render;
    formats.reverse_output_stream() = state->render;
    if (state->processor->Initialize(formats) != 0) return nullptr;
    return state.release();
  } catch (...) { return nullptr; }
}

void openkrisp_aec_destroy(void* state) noexcept {
  delete static_cast<OpenKrispAec*>(state);
}

int openkrisp_aec_render(void* handle, const float* left, const float* right) noexcept {
  auto* state = static_cast<OpenKrispAec*>(handle);
  const float* planes[2] = {left, right};
  try { return state->processor->AnalyzeReverseStream(planes, state->render); }
  catch (...) { return -1; }
}

int openkrisp_aec_capture(void* handle, const float* input, float* output) noexcept {
  auto* state = static_cast<OpenKrispAec*>(handle);
  const float* source[1] = {input};
  float* destination[1] = {output};
  try { return state->processor->ProcessStream(source, state->capture, state->capture, destination); }
  catch (...) { return -1; }
}
}
