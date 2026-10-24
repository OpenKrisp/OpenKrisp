import { useI18n } from "./i18n";
import type { RouteSnapshot } from "./api";
import type { RouteDraft } from "./drafts";
import { noiseReductionEnabled } from "./AudioControls";
import { CallSourceIllustration, FilterIllustration, FlowArrow, PlaybackIllustration } from "./FlowIllustrations";

export function CallAudioFlow({ draft, route, running, waiting }: {
  draft: RouteDraft; route: RouteSnapshot | undefined; running: boolean; waiting: boolean;
}) {
  const { t } = useI18n();
  const enabled = noiseReductionEnabled(draft, route);
  const paused = running && enabled && route?.metrics.degraded;
  const reducing = enabled && !paused && !waiting;
  const processing = waiting ? t("flow.waiting") : paused ? t("flow.paused")
    : !enabled ? t("flow.original") : running ? t("flow.reducing") : t("flow.reduces");

  return <ol className={`microphone-flow call-audio-flow ${running ? "is-running" : ""}`} aria-label={t("flow.speakers")}>
    <li><CallSourceIllustration /><FlowArrow /><strong>{t("flow.callApp")}</strong><span>{t("flow.voices")}</span></li>
    <li><FilterIllustration reducing={reducing} /><FlowArrow /><strong>OpenKrisp</strong><span>{processing}</span></li>
    <li><PlaybackIllustration reducing={reducing} /><strong>{t("flow.hear")}</strong><span>{t("flow.playback")}</span></li>
  </ol>;
}
