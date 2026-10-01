import type { RouteSnapshot } from "./api";
import type { RouteDraft } from "./drafts";
import { noiseReductionEnabled } from "./AudioControls";
import { CallSourceIllustration, FilterIllustration, FlowArrow, PlaybackIllustration } from "./FlowIllustrations";

export function CallAudioFlow({ draft, route, running, waiting }: {
  draft: RouteDraft; route: RouteSnapshot | undefined; running: boolean; waiting: boolean;
}) {
  const enabled = noiseReductionEnabled(draft, route);
  const paused = running && enabled && route?.metrics.degraded;
  const reducing = enabled && !paused && !waiting;
  const processing = waiting ? "Waiting for audio" : paused ? "Noise reduction paused"
    : !enabled ? "Original sound" : running ? "Reducing the noise" : "Reduces the noise";

  return <ol className={`microphone-flow call-audio-flow ${running ? "is-running" : ""}`} aria-label="Your speakers flow">
    <li><CallSourceIllustration /><FlowArrow /><strong>Your call app</strong><span>Voices + room sound</span></li>
    <li><FilterIllustration reducing={reducing} /><FlowArrow /><strong>OpenKrisp</strong><span>{processing}</span></li>
    <li><PlaybackIllustration reducing={reducing} /><strong>You hear</strong><span>Headphones or speakers</span></li>
  </ol>;
}
