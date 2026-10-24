import { useI18n } from "./i18n";
import type { RouteSnapshot } from "./api";
import type { RouteDraft } from "./drafts";
import { noiseReductionEnabled } from "./AudioControls";

import { CallIllustration, FilterIllustration, FlowArrow, MicrophoneIllustration } from "./FlowIllustrations";

/** A schematic of the selected route; the input meter is the live audio measurement. */
export function MicrophoneFlow({ draft, route, running, waiting }: {
  draft: RouteDraft; route: RouteSnapshot | undefined; running: boolean; waiting: boolean;
}) {
  const { t } = useI18n();
  const enabled = noiseReductionEnabled(draft, route);
  const paused = running && enabled && route?.metrics.degraded;
  const reducing = enabled && !paused && !waiting;
  const headphones = draft.destinationKind === "physical_output";
  const processing = waiting ? t("flow.waiting") : paused ? t("flow.paused")
    : !enabled ? t("flow.original") : running ? t("flow.reducing") : t("flow.reduces");

  return <ol className={`microphone-flow ${running ? "is-running" : ""}`} aria-label={t("flow.microphone")}>
    <li>
      <MicrophoneIllustration /><FlowArrow />
      <strong>{t("microphone.title")}</strong><span>{t("flow.roomSound")}</span>
    </li>
    <li>
      <FilterIllustration reducing={reducing} /><FlowArrow />
      <strong>OpenKrisp</strong><span>{processing}</span>
    </li>
    <li>
      <CallIllustration headphones={headphones} reducing={reducing} />
      <strong>{headphones ? t("flow.headphones") : t("flow.virtualMicrophone")}</strong>
      <span>{headphones ? t("flow.listen") : t("flow.forCall")}</span>
    </li>
  </ol>;
}
