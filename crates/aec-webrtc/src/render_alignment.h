#ifndef OPENKRISP_RENDER_ALIGNMENT_H_
#define OPENKRISP_RENDER_ALIGNMENT_H_

#include <algorithm>
#include "modules/audio_processing/aec3/block.h"

namespace openkrisp {

// Delay-estimation sidechain only: the adaptive echo filters retain both original
// render planes. Use the stereo average unless cancellation removes more than
// half the power expected for uncorrelated channels; then select the stronger
// original channel. Stateless and bounded, including silence and channel changes.
inline void PreserveAlignmentExcitation(
    const webrtc::Block& render,
    rtc::ArrayView<float, webrtc::kBlockSize> mixed) {
  if (render.NumChannels() != 2) return;
  float left_power = 0, right_power = 0, mixed_power = 0;
  const auto left = render.View(/*band=*/0, /*channel=*/0);
  const auto right = render.View(/*band=*/0, /*channel=*/1);
  for (size_t i = 0; i < mixed.size(); ++i) {
    left_power += left[i] * left[i];
    right_power += right[i] * right[i];
    mixed_power += mixed[i] * mixed[i];
  }
  if (mixed_power < .125f * (left_power + right_power)) {
    const auto selected = left_power >= right_power ? left : right;
    std::copy(selected.begin(), selected.end(), mixed.begin());
  }
}

}  // namespace openkrisp
#endif
