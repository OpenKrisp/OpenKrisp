import type { RouteSnapshot } from "./api";
import type { RouteDraft } from "./drafts";
import { noiseReductionEnabled } from "./AudioControls";

import { CallIllustration, FilterIllustration, FlowArrow, MicrophoneIllustration } from "./FlowIllustrations";

/** A schematic of the selected route; the input meter is the live audio measurement. */
export function MicrophoneFlow({ draft, route, running, waiting }: {
  draft: RouteDraft; route: RouteSnapshot | undefined; running: boolean; waiting: boolean;
}) {
  const enabled = noiseReductionEnabled(draft, route);
  const paused = running && enabled && route?.metrics.degraded;
  const reducing = enabled && !paused && !waiting;
  const headphones = draft.destinationKind === "physical_output";
  const processing = waiting ? "Waiting for audio" : paused ? "Noise reduction paused"
    : !enabled ? "Original sound" : running ? "Reducing the noise" : "Reduces the noise";

  return <ol className={`microphone-flow ${running ? "is-running" : ""}`} aria-label="Microphone audio flow">
    <li>
      <MicrophoneIllustration /><FlowArrow />
      <strong>Your microphone</strong><span>Voice + room sound</span>
    </li>
    <li>
      <FilterIllustration reducing={reducing} /><FlowArrow />
      <strong>OpenKrisp</strong><span>{processing}</span>
    </li>
    <li>
      <CallIllustration headphones={headphones} reducing={reducing} />
      <strong>{headphones ? "Your headphones" : "OpenKrisp microphone"}</strong>
      <span>{headphones ? "Listen to yourself" : "For your call app"}</span>
    </li>
  </ol>;
}
