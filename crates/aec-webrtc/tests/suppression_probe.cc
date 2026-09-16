// Standalone quality diagnostic, never linked into the application. The two export
// flags below expose AEC3's 16 kHz linear output before its residual suppressor.
// The production adapter exports neither PCM nor a tuning interface.
#include "webrtc/api/audio/audio_processing.h"
#include "webrtc/modules/audio_processing/aec3/echo_canceller3.h"
#include "webrtc/modules/audio_processing/aec3/alignment_mixer.h"
#include "webrtc/rtc_base/logging.h"
#include "../src/config.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <stdexcept>
#include <string_view>
#include <utility>
#include <vector>

namespace {
constexpr int kSamples = 480;
constexpr int kFrames = 1000;
using Frame = std::array<float, kSamples>;
enum class Mode { AutomaticChannels, ExplicitChannels, StationarityTrial, MaskingTrial, PrototypeProfile };

class Factory final : public webrtc::EchoControlFactory {
 public:
  explicit Factory(Mode mode) : mode_(mode) {}
  std::unique_ptr<webrtc::EchoControl> Create(int rate, int render, int capture) override {
    webrtc::EchoCanceller3Config config;
    auto stereo = webrtc::EchoCanceller3::CreateDefaultMultichannelConfig();
    if (mode_ == Mode::PrototypeProfile) openkrisp::ConfigureAec3(config, stereo);
    for (auto* c : {&config, &stereo}) {
      c->multi_channel.detect_stereo_content = mode_ == Mode::AutomaticChannels;
      // APM's export flag alone does not initialize AEC3's linear block framer in
      // this revision. Both flags are required in this diagnostic-only factory.
      c->filter.export_linear_aec_output = true;
      if (mode_ == Mode::StationarityTrial) {
        c->echo_audibility.use_stationarity_properties = true;
      }
      if (mode_ == Mode::MaskingTrial) {
        c->suppressor.nearend_tuning.mask_hf = c->suppressor.nearend_tuning.mask_lf;
        c->erle.max_h = c->erle.max_l;
      }
    }
    return std::make_unique<webrtc::EchoCanceller3>(config, stereo, rate, render, capture);
  }
 private:
  Mode mode_;
};

auto MakeProcessor(Mode mode, int channels = 2) {
  auto p = webrtc::AudioProcessingBuilder()
               .SetEchoControlFactory(std::make_unique<Factory>(mode)).Create();
  if (!p) throw std::runtime_error("APM construction failed");
  webrtc::AudioProcessing::Config c;
  c.pipeline.multi_channel_render = true;
  c.pipeline.multi_channel_capture = false;
  c.echo_canceller.enabled = true;
  c.echo_canceller.mobile_mode = false;
  c.echo_canceller.enforce_high_pass_filtering = false;
  c.echo_canceller.export_linear_aec_output = true;
  c.high_pass_filter.enabled = true;
  c.noise_suppression.enabled = false;
  c.gain_controller1.enabled = false;
  c.gain_controller2.enabled = false;
  c.pre_amplifier.enabled = false;
  c.capture_level_adjustment.enabled = false;
  p->ApplyConfig(c);
  webrtc::ProcessingConfig f;
  f.input_stream() = webrtc::StreamConfig(48000, 1);
  f.output_stream() = f.input_stream();
  f.reverse_input_stream() = webrtc::StreamConfig(48000, channels);
  f.reverse_output_stream() = f.reverse_input_stream();
  if (p->Initialize(f)) throw std::runtime_error("APM initialization failed");
  return p;
}

std::vector<float> Signal(uint32_t seed) {
  std::vector<float> result(kSamples * kFrames);
  float previous = 0;
  for (float& value : result) {
    seed = seed * 1664525u + 1013904223u;
    previous = .7f * previous + .3f * (float(int32_t(seed)) / float(INT32_MAX));
    value = .2f * previous;
  }
  return result;
}

struct Score {
  double reference = 0, output = 0, projection = 0;
  void Add(double r, double o) {
    if (!std::isfinite(r) || !std::isfinite(o)) throw std::runtime_error("nonfinite PCM");
    reference += r * r; output += o * o; projection += r * o;
  }
  double Gain() const { return projection / reference; }
  double Correlation() const { return projection / std::sqrt(reference * output); }
};

void Run(Mode mode, const char* label) {
  auto far = Signal(739), near = Signal(541);
  auto aec = MakeProcessor(mode), baseline = MakeProcessor(mode);
  Frame left{}, right{}, mic{}, output{}, near_input{}, near_output{}, zero{};
  std::array<std::array<float, 160>, 1> linear{}, near_linear{};
  Score full_score, linear_score;
  double far_input = 0, far_output = 0;
  for (int frame = 0; frame < kFrames; ++frame) {
    for (int i = 0; i < kSamples; ++i) {
      const int n = frame * kSamples + i;
      left[i] = far[n]; right[i] = -far[n];
      near_input[i] = frame >= 600 ? near[n] : 0;
      mic[i] = (n >= 4800 ? .7f * far[n - 4800] : 0) + near_input[i];
    }
    const float* render[] = {left.data(), right.data()};
    const float* quiet[] = {zero.data(), zero.data()};
    const float* capture[] = {mic.data()};
    const float* near_capture[] = {near_input.data()};
    float* destination[] = {output.data()};
    float* near_destination[] = {near_output.data()};
    const webrtc::StreamConfig mono(48000, 1), stereo(48000, 2);
    if (aec->AnalyzeReverseStream(render, stereo) ||
        aec->ProcessStream(capture, mono, mono, destination) ||
        baseline->AnalyzeReverseStream(quiet, stereo) ||
        baseline->ProcessStream(near_capture, mono, mono, near_destination)) {
      throw std::runtime_error("APM processing failed");
    }
    if (!aec->GetLinearAecOutput(linear) || !baseline->GetLinearAecOutput(near_linear)) {
      throw std::runtime_error("linear output unavailable");
    }
    if (frame >= 400 && frame < 600) {
      for (int i = 0; i < kSamples; ++i) {
        far_input += double(mic[i]) * mic[i];
        far_output += double(output[i]) * output[i];
      }
    }
    if (frame >= 700) {
      // Each output is compared with its own same-stage, near-only APM baseline.
      // Never compare the 16 kHz linear output directly with the 48 kHz final PCM.
      for (int i = 0; i < kSamples; ++i) full_score.Add(near_output[i], output[i]);
      for (int i = 0; i < 160; ++i) linear_score.Add(near_linear[0][i], linear[0][i]);
    }
  }
  const double erle = 10 * std::log10(far_input / far_output);
  if (!std::isfinite(erle) || !std::isfinite(full_score.Gain()) ||
      !std::isfinite(full_score.Correlation()) || !std::isfinite(linear_score.Gain()) ||
      !std::isfinite(linear_score.Correlation())) throw std::runtime_error("invalid score");
  std::printf("mode=%s ERLE_db=%.6f output_gain=%.6f output_correlation=%.6f linear_gain=%.6f linear_correlation=%.6f\n",
              label, erle, full_score.Gain(), full_score.Correlation(),
              linear_score.Gain(), linear_score.Correlation());
}

std::vector<float> Speech(uint32_t seed, double pitch, double period) {
  auto result = Signal(seed);
  for (size_t n = 0; n < result.size(); ++n) {
    const double t = double(n) / 48000;
    const double syllable = std::fmod(t, period) / period;
    if (syllable >= .15 && syllable < .7) {
      const double phase = 6.283185307179586 * pitch * t + .5 * std::sin(t * 5);
      double harmonic = 0;
      for (int h = 1; h <= 8; ++h) harmonic += std::sin(phase * h) / h;
      result[n] = float(.08 * std::min({(syllable - .15) * 30, (.7 - syllable) * 30, 1.}) * harmonic);
    } else if (syllable >= .85) {
      result[n] = 0;
    }
  }
  return result;
}

// Separate a delay change from a gain change before considering tuning. Linear
// echo reduction uses a paired, same-stage baseline; APM delay_ms is a software
// render-buffer estimate, not the fixture delay or measured end-to-end latency.
void RunPathChange(int channels, bool speech, bool delay_change, bool gain_change) {
  auto left = speech ? Speech(739, 117, .43) : Signal(739);
  auto right = speech ? Speech(91, 237, .71) : Signal(91);
  auto aec = MakeProcessor(Mode::PrototypeProfile, channels);
  auto baseline = MakeProcessor(Mode::PrototypeProfile, channels);
  Frame l{}, r{}, mic{}, output{}, baseline_output{}, zero{};
  std::array<std::array<float, 160>, 1> linear{}, baseline_linear{};
  double input_energy = 0, output_energy = 0, linear_input = 0, linear_output = 0;
  int min_delay = 100000, max_delay = -1, delay_samples = 0;
  const webrtc::StreamConfig mono(48000, 1), render_format(48000, channels);
  for (int frame = 0; frame < kFrames; ++frame) {
    const int delay = delay_change && frame >= 600 ? 7680 : 4800;
    const float gain = gain_change && frame >= 600 ? .35f : .65f;
    for (int i = 0; i < kSamples; ++i) {
      const int n = frame * kSamples + i;
      l[i] = left[n]; r[i] = right[n];
      mic[i] = n >= delay + 31 ? gain * left[n - delay] +
          (channels == 2 ? .3f * right[n - delay - 31] : 0) : 0;
    }
    const float* render[] = {l.data(), r.data()};
    const float* quiet[] = {zero.data(), zero.data()};
    const float* capture[] = {mic.data()};
    float* destination[] = {output.data()};
    float* baseline_destination[] = {baseline_output.data()};
    if (aec->AnalyzeReverseStream(render, render_format) ||
        aec->ProcessStream(capture, mono, mono, destination) ||
        baseline->AnalyzeReverseStream(quiet, render_format) ||
        baseline->ProcessStream(capture, mono, mono, baseline_destination) ||
        !aec->GetLinearAecOutput(linear) || !baseline->GetLinearAecOutput(baseline_linear)) {
      throw std::runtime_error("path probe processing failed");
    }
    // Drain APM's capacity-one stats queue from the first frame. Starting to
    // query at the measurement window would first return stale startup stats.
    auto metrics = aec->GetStatistics();
    if (frame < 400) continue;
    for (int i = 0; i < kSamples; ++i) {
      input_energy += double(mic[i]) * mic[i];
      output_energy += double(output[i]) * output[i];
    }
    for (int i = 0; i < 160; ++i) {
      linear_input += double(baseline_linear[0][i]) * baseline_linear[0][i];
      linear_output += double(linear[0][i]) * linear[0][i];
    }
    if (metrics.delay_ms) {
      min_delay = std::min(min_delay, *metrics.delay_ms);
      max_delay = std::max(max_delay, *metrics.delay_ms);
      ++delay_samples;
    }
    if ((frame + 1) % 200 == 0) {
      const double erle = 10 * std::log10(input_energy / output_energy);
      const double linear_erle = 10 * std::log10(linear_input / linear_output);
      if (!std::isfinite(erle) || !std::isfinite(linear_erle) || delay_samples != 200) {
        throw std::runtime_error("invalid path-probe measurement");
      }
      std::printf("path channels=%d speech=%d delay_change=%d gain_change=%d window_end_s=%d ERLE_db=%.6f linear_ERLE_db=%.6f buffer_delay_min_ms=%d buffer_delay_max_ms=%d\n",
                  channels, speech, delay_change, gain_change, (frame + 1) / 100,
                  erle, linear_erle, min_delay, max_delay);
      input_energy = output_energy = linear_input = linear_output = 0;
      min_delay = 100000; max_delay = -1; delay_samples = 0;
    }
  }
}

void CheckAlignmentMixing() {
  webrtc::EchoCanceller3Config mono;
  auto stereo = webrtc::EchoCanceller3::CreateDefaultMultichannelConfig();
  openkrisp::ConfigureAec3(mono, stereo);
  if (!webrtc::EchoCanceller3Config::Validate(&mono) ||
      !webrtc::EchoCanceller3Config::Validate(&stereo)) {
    throw std::runtime_error("prototype config is outside upstream bounds");
  }
  webrtc::AlignmentMixer mixer(2, stereo.delay.render_alignment_mixing);
  webrtc::Block render(1, 2);
  std::array<float, webrtc::kBlockSize> mixed{};
  // Reuse the actual patched native mixer through format content transitions.
  // Case 6 is silence after signal, so stale sidechain content is observable.
  for (int scenario = 0; scenario < 7; ++scenario) {
    for (size_t i = 0; i < mixed.size(); ++i) {
      const float left = i % 2 ? 1000.f : -1000.f;
      render.View(0, 0)[i] = scenario == 3 || scenario == 6 ? 0 : left;
      render.View(0, 1)[i] = scenario == 0 ? left :
          scenario == 1 ? -left : scenario == 2 ? -1.1f * left :
          scenario == 3 ? left : scenario == 4 || scenario == 6 ? 0 :
          i % 4 < 2 ? 1000.f : -1000.f;
    }
    const auto original = render;
    mixer.ProduceOutput(render, mixed);
    for (size_t i = 0; i < mixed.size(); ++i) {
      const float left = original.View(0, 0)[i], right = original.View(0, 1)[i];
      const float expected = scenario == 1 ? left : scenario == 2 ? right : (left + right) * .5f;
      if (mixed[i] != expected || render.View(0, 0)[i] != left || render.View(0, 1)[i] != right) {
        throw std::runtime_error("native alignment mix changed reference planes or lost excitation");
      }
    }
  }
  std::puts("native_alignment_contracts=passed cases=7");
}
}  // namespace

int main(int argc, char** argv) {
  rtc::LogMessage::LogToDebug(rtc::LS_NONE);
  rtc::LogMessage::SetLogToStderr(false);
  try {
    CheckAlignmentMixing();
    if (argc == 2 && std::string_view(argv[1]) == "--alignment") return 0;
    if (argc == 2 && std::string_view(argv[1]) == "--path-changes") {
      for (int channels : {1, 2}) {
        for (bool speech : {false, true}) {
          for (auto changes : {std::pair{false, false}, std::pair{false, true},
                               std::pair{true, false}, std::pair{true, true}}) {
            RunPathChange(channels, speech, changes.first, changes.second);
          }
        }
      }
      return 0;
    }
    if (argc != 1) throw std::runtime_error("usage: suppression-probe [--path-changes|--alignment]");
    Run(Mode::AutomaticChannels, "previous_automatic_channels");
    Run(Mode::ExplicitChannels, "explicit_channels");
    Run(Mode::StationarityTrial, "stationarity_trial_not_adopted");
    Run(Mode::MaskingTrial, "masking_trial_not_adopted");
    Run(Mode::PrototypeProfile, "current_prototype_profile");
  } catch (const std::exception& error) {
    std::fprintf(stderr, "quality probe failed: %s\n", error.what());
    return 1;
  }
}
