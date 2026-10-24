import { Message, useI18n } from "./i18n";
import type { RoutingSnapshot, PlatformEndpoint } from "./api";
import { AudioIcon } from "./AudioControls";

function needsSetup(endpoint: PlatformEndpoint) {
  return endpoint.availability !== "ready" && !(endpoint.availability === "missing" && endpoint.canCreate);
}

function ConnectionGuide({ kind, supported, endpoints }: {
  kind: "microphone" | "speaker"; supported: boolean; endpoints: PlatformEndpoint[];
}) {
  const { t } = useI18n();
  const microphone = kind === "microphone";
  const titleId = microphone ? "setup-title" : "speaker-help-title";
  const endpoint = endpoints.length === 1 ? endpoints[0] : undefined;

  return <section className="connection-guide" aria-labelledby={titleId}>
    <div className="help-section-heading"><AudioIcon kind={microphone ? "microphone" : "headphones"} />
      <h2 id={titleId}>{t(microphone ? "setup.microphoneTitle" : "setup.speakerTitle")}</h2></div>
    {!supported ? <p>{t(microphone ? "setup.microphoneUnsupported" : "setup.speakerUnsupported")}</p>
      : !endpoint ? <p>{endpoints.length === 0
        ? t(microphone ? "setup.microphoneMissing" : "setup.speakerMissing")
        : t(microphone ? "setup.microphoneMultiple" : "setup.speakerMultiple")}</p>
      : needsSetup(endpoint) ? <p className="notice"><strong>{endpoint.name}</strong> {t(endpoint.availability === "missing" ? "setup.install" : "setup.repair")} {t("setup.reinstall")}</p>
      : <>
          <ol className="connection-steps" role="list">
            <li><span>{t(microphone ? "setup.chooseMicrophone" : "setup.chooseSpeakers")}</span></li>
            <li><span>{t("common.startStep")}</span></li>
            <li><span><Message id={microphone ? "setup.selectMicrophone" : "setup.selectSpeaker"} values={{ device: <strong>{endpoint.name}</strong> }} /></span></li>
          </ol>
          <p className="connection-note"><AudioIcon kind="headphones" /><span>{t(microphone ? "setup.keepPlayback" : "common.switchBack")}</span></p>
        </>}
  </section>;
}

export function EndpointSetup({ routing }: { routing: RoutingSnapshot }) {
  return <div className="connection-guides">
    <ConnectionGuide kind="microphone" supported={routing.capabilities.virtualMicrophone} endpoints={routing.endpoints} />
    <ConnectionGuide kind="speaker" supported={routing.capabilities.systemLoopback} endpoints={routing.loopbackEndpoints ?? []} />
  </div>;
}
