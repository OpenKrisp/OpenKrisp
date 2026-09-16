#ifndef OPENKRISP_AEC_CONFIG_H_
#define OPENKRISP_AEC_CONFIG_H_

#include <initializer_list>
#include "webrtc/api/audio/echo_canceller3_config.h"

namespace openkrisp {

// Worker-owned prototype profile. These are estimator/suppressor parameters,
// not relaxed acceptance thresholds or measured room/device characteristics.
inline void ConfigureAec3(webrtc::EchoCanceller3Config& mono,
                         webrtc::EchoCanceller3Config& stereo) {
  for (auto* config : {&mono, &stereo}) {
    // Route formats are fixed for a generation. Content-based mono downmix can
    // cancel opposite-phase playback indefinitely when speech contains pauses.
    config->multi_channel.detect_stereo_content = false;
    // Let measured linear-filter convergence exceed the upstream 4/1.5 power
    // ratio caps. The estimate still starts at 1 and follows observed error.
    config->erle.max_l = 16.f;
    config->erle.max_h = 16.f;
    // Preserve unvoiced local speech when local activity has been detected.
    // Normal suppressor masking thresholds and saturation handling stay upstream.
    config->suppressor.nearend_tuning.mask_hf =
        webrtc::EchoCanceller3Config::Suppressor::MaskingThresholds(1.09f, 1.29f, .3f);
    config->suppressor.dominant_nearend_detection.enr_threshold = 2.f;
    config->suppressor.dominant_nearend_detection.trigger_threshold = 3;
    // Before a usable linear estimate exists, avoid assuming unity acoustic
    // coupling: that can erase local speech even when no echo reaches the mic.
    config->ep_strength.default_gain = .05f;
  }
  // Estimate delay from both speakers, with the bounded non-cancelling mix in
  // render_alignment.h. A single-channel echo can explain half this sidechain;
  // allow unequal speaker coupling while retaining upstream lag-voting,
  // excitation and delay-change hysteresis. Mono keeps its 0.2 criterion.
  stereo.delay.render_alignment_mixing.downmix = true;
  stereo.delay.render_alignment_mixing.adaptive_selection = false;
  stereo.delay.delay_candidate_detection_threshold = .6f;
}

}  // namespace openkrisp
#endif
