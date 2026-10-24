import { useI18n, Message, ErrorDetails } from "./i18n";
import { useEffect, useRef, useState } from "react";
import { api, type ApiError, type EngineSnapshot } from "./api";
import { incomingSelectionError } from "./incoming";
import { isPipeWireDefaultAlias, routeCanStop, routeIsActive, routeToStop } from "./routing";
import { normalizeApiError } from "./state";
import { automaticDraft } from "./preferences";
import { AudioIcon, AudioLevel, audioStatus } from "./AudioControls";
import { CallAudioFlow } from "./CallAudioFlow";
import { draftSettingsError, requestFromDraft, type RouteDraft } from "./drafts";

export function IncomingAudio({ snapshot, connected, draft, onDraft, settingsReady, helpOpen = false, onOpenHelp }: {
  snapshot: EngineSnapshot; connected: boolean; draft: RouteDraft; onDraft: (draft: RouteDraft) => void;
  settingsReady: boolean; helpOpen?: boolean; onOpenHelp?: () => void;
}) {
  const { t, error: displayError } = useI18n();
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [pending, setPending] = useState<number | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const route = snapshot.routes.find((route) => !("physicalInput" in route.config.source));
  const active = routeIsActive(route);
  const locked = busy || !connected || snapshot.shuttingDown;
  const hasStarted = active || pending !== null;
  const starting = route?.state === "starting" || (pending !== null && !active);
  const actionLabel = !hasStarted ? t("common.turnOn") : route?.state === "stopping" ? t("common.turningOff")
    : starting ? t("common.cancelStart") : t("common.turnOff");
  const editingLocked = locked || !settingsReady || hasStarted;
  const selection = hasStarted ? draft : automaticDraft(draft, snapshot);
  const startSelection: RouteDraft = { ...selection, mode: "speech_denoise", denoise: { ...selection.denoise, enabled: true } };
  const running = connected && route?.state === "running";
  const { inputId: sourceId, outputId } = selection;
  const source = snapshot.routing.loopbackEndpoints?.find((endpoint) => endpoint.id === sourceId);
  const outputs = snapshot.inventory.devices.filter((device) => device.supportsOutput && device.kind === "physical" && !isPipeWireDefaultAlias(device.id));
  const validation = incomingSelectionError(snapshot, sourceId, outputId, t) ?? draftSettingsError(selection, t);

  const rawFault = error ?? route?.fault;
  const fault = rawFault ? displayError(rawFault) : null;

  useEffect(() => {
    if (pending !== null && snapshot.revision > pending && (active || snapshot.lastFault)) setPending(null);
  }, [snapshot, active, pending]);

  async function run(action: () => Promise<unknown>) {
    if (busyRef.current) return;
    busyRef.current = true; setBusy(true); setError(null);
    try { await action(); }
    catch (failure) { setError(normalizeApiError(failure)); setPending(null); }
    finally { busyRef.current = false; setBusy(false); }
  }

  function stop() {
    const routeId = routeToStop("incoming", route, pending !== null);
    if (!routeId) return;
    void run(async () => { await api.stopRoute(routeId); setPending(null); });
  }

  return <>
    <section className="audio-panel incoming-panel" aria-labelledby="speakers-title" hidden={helpOpen}>
      <div className="audio-panel-heading"><h2 id="speakers-title">{t("speakers.title")}</h2>
        <span className={`status ${running ? "on" : ""}`} role="status"><span className={starting && connected ? "status-spinner" : "status-dot"} aria-hidden="true" />{audioStatus(route, connected, pending !== null, t)}</span></div>
      <p className="audio-panel-description">{t("speakers.description")}</p>
      <div className="incoming-content" role="group" aria-label={t("speakers.controls")}>
        <CallAudioFlow draft={hasStarted ? selection : startSelection} route={route} running={running}
          waiting={!connected || !settingsReady || (hasStarted && !running)} />
        <div className="device-heading"><label htmlFor="playback-device">{t("speakers.device")}</label></div>
          <select id="playback-device" disabled={editingLocked} aria-describedby={hasStarted ? "playback-locked" : undefined} value={outputId} onChange={(event) => onDraft({ ...selection, outputId: event.target.value })}>
            <option value="">{t("speakers.choose")}</option>
            {outputId && !outputs.some((output) => output.id === outputId) ? <option value={outputId}>{t("speakers.unavailable")}</option> : null}
            {outputs.map((output) => <option key={output.id} value={output.id}>{output.name}</option>)}
          </select>
        {hasStarted ? <p id="playback-locked" className="device-hint">{starting && connected ? t("common.preparing") : t("common.changeDevices")}</p> : null}
        <AudioLevel label={t("speakers.level")} value={route?.metrics.outputPeak ?? 0} active={running} />
        {running && route?.metrics.degraded ? <p className="notice" role="status">{t("common.degraded")}</p> : null}
        {!active && validation ? <div className="notice"><p>{validation}</p>{onOpenHelp ? <button className="text-button" onClick={onOpenHelp}>{t("common.setupHelp")}</button> : null}</div> : null}
        <button className={`primary transport ${hasStarted ? "transport-stop" : ""}`} disabled={locked || (hasStarted ? !routeCanStop(route, pending !== null) : !settingsReady || validation !== null)} onClick={hasStarted ? stop : () => {
          onDraft(startSelection); setPending(snapshot.revision); void run(() => api.startRoute(requestFromDraft(startSelection)));
        }}><span className={hasStarted ? "stop-icon" : "start-icon"} aria-hidden="true" />{actionLabel}</button>
        {route?.state === "failed" ? <button className="text-button cleanup-button" disabled={locked} onClick={stop}>{t("speakers.release")}</button> : null}
        {fault && rawFault ? <div className="notice error" role="alert"><p>{fault.message}</p><ErrorDetails error={rawFault} /></div> : null}
        {source ? <section className="call-setup" aria-labelledby="speaker-setup-title">
          <AudioIcon kind="headphones" className="handoff-icon" />
          <div className="call-setup-content">
            <h3 id="speaker-setup-title">{t("speakers.callDevice")}</h3>
            <p className="call-device-name">{source.name}</p>
            <details className="setup-help"><summary>{t("common.connect")}</summary><ol>
              <li>{t("speakers.chooseStep")}</li>
              <li>{t("speakers.openSettings")}</li>
              <li><Message id="speakers.select" values={{ device: <strong>{source.name}</strong> }} /></li>
            </ol><p>{t("speakers.headphones")}</p>
            <p>{t("common.switchBack")}</p></details>
          </div>
        </section> : null}
        {hasStarted ? <p className="hint call-audio-reminder">{t("common.switchBack")}</p> : null}
      </div>
    </section>
    <section className="settings-section" aria-labelledby="incoming-title" hidden={!helpOpen || (!hasStarted && !error && !route?.fault)}>
      <div className="panel-heading"><h2 id="incoming-title">{t("speakers.title")}</h2>
        {hasStarted ? <button disabled={locked || !routeCanStop(route, pending !== null)} onClick={stop}>{actionLabel}</button> : null}</div>
      {hasStarted ? <p className="settings-playback-status" role="status">{t("speakers.status", { status: audioStatus(route, connected, pending !== null, t) })}</p> : null}
      {fault && rawFault ? <div className="notice error" role="alert"><p>{fault.message}</p><ErrorDetails error={rawFault} /></div> : null}
    </section>
  </>;
}
