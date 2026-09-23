import { useEffect, useRef, useState } from "react";
import { api, type EngineSnapshot } from "./api";
import { incomingRouteId, incomingSelectionError } from "./incoming";
import { isPipeWireDefaultAlias, routeCanStop, routeIsActive } from "./routing";
import { normalizeApiError } from "./state";
import { automaticDraft } from "./preferences";
import { AudioIcon, AudioLevel, audioStatus } from "./AudioControls";
import { CallAudioFlow } from "./CallAudioFlow";
import { draftSettingsError, requestFromDraft, type RouteDraft } from "./drafts";

export function IncomingAudio({ snapshot, connected, draft, onDraft, settingsReady, helpOpen = false, onOpenHelp }: {
  snapshot: EngineSnapshot; connected: boolean; draft: RouteDraft; onDraft: (draft: RouteDraft) => void;
  settingsReady: boolean; helpOpen?: boolean; onOpenHelp?: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [pending, setPending] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const route = snapshot.routes.find((route) => !("physicalInput" in route.config.source));
  const active = routeIsActive(route);
  const locked = busy || !connected || snapshot.shuttingDown;
  const hasStarted = active || pending !== null;
  const editingLocked = locked || !settingsReady || hasStarted;
  const selection = hasStarted ? draft : automaticDraft(draft, snapshot);
  const startSelection: RouteDraft = { ...selection, mode: "speech_denoise", denoise: { ...selection.denoise, enabled: true } };
  const running = connected && route?.state === "running";
  const { inputId: sourceId, outputId } = selection;
  const source = snapshot.routing.loopbackEndpoints?.find((endpoint) => endpoint.id === sourceId);
  const outputs = snapshot.inventory.devices.filter((device) => device.supportsOutput && device.kind === "physical" && !isPipeWireDefaultAlias(device.id));
  const validation = incomingSelectionError(snapshot, sourceId, outputId) ?? draftSettingsError(selection);

  useEffect(() => {
    if (pending !== null && snapshot.revision > pending && (active || snapshot.lastFault)) setPending(null);
  }, [snapshot, active, pending]);

  async function run(action: () => Promise<unknown>) {
    if (busyRef.current) return;
    busyRef.current = true; setBusy(true); setError(null);
    try { await action(); }
    catch (failure) { setError(normalizeApiError(failure).message); setPending(null); }
    finally { busyRef.current = false; setBusy(false); }
  }

  function stop() {
    void run(async () => { await api.stopRoute(pending !== null ? incomingRouteId : route?.config.id ?? incomingRouteId); setPending(null); });
  }

  return <>
    <section className="audio-panel incoming-panel" aria-labelledby="speakers-title" hidden={helpOpen}>
      <div className="audio-panel-heading"><h2 id="speakers-title">Your speakers</h2>
        <span className={`status ${running ? "on" : ""}`} role="status"><span className="status-dot" />{audioStatus(route, connected, pending !== null)}</span></div>
      <p className="audio-panel-description">Reduce background noise from other people in your call.</p>
      <div className="incoming-content" role="group" aria-label="Your speakers controls">
        <CallAudioFlow draft={hasStarted ? selection : startSelection} route={route} running={running}
          waiting={!connected || !settingsReady || (hasStarted && !running)} />
        <div className="device-heading"><label htmlFor="playback-device">Play through</label></div>
          <select id="playback-device" disabled={editingLocked} aria-describedby={hasStarted ? "playback-locked" : undefined} value={outputId} onChange={(event) => onDraft({ ...selection, outputId: event.target.value })}>
            <option value="">Choose headphones or speakers</option>
            {outputId && !outputs.some((output) => output.id === outputId) ? <option value={outputId}>Saved headphones or speakers unavailable</option> : null}
            {outputs.map((output) => <option key={output.id} value={output.id}>{output.name}</option>)}
          </select>
        {hasStarted ? <p id="playback-locked" className="device-hint">Turn off OpenKrisp to change devices.</p> : null}
        <AudioLevel label="Playback level" value={route?.metrics.outputPeak ?? 0} active={running} />
        {running && route?.metrics.degraded ? <p className="notice" role="status">Noise reduction is temporarily paused to keep your audio playing.</p> : null}
        {!active && validation ? <div className="notice"><p>{validation}</p>{onOpenHelp ? <button className="text-button" onClick={onOpenHelp}>Get setup help</button> : null}</div> : null}
        <button className={`primary transport ${hasStarted ? "transport-stop" : ""}`} disabled={locked || (hasStarted ? !routeCanStop(route, pending !== null) : !settingsReady || validation !== null)} onClick={hasStarted ? stop : () => {
          onDraft(startSelection); setPending(snapshot.revision); void run(() => api.startRoute(requestFromDraft(startSelection)));
        }}><span className={hasStarted ? "stop-icon" : "start-icon"} aria-hidden="true" />{hasStarted ? route?.state === "stopping" ? "Turning off OpenKrisp…" : pending !== null && !active ? "Cancel start" : "Turn off OpenKrisp" : "Turn on OpenKrisp"}</button>
        {route?.state === "failed" ? <button className="text-button cleanup-button" disabled={locked} onClick={stop}>Release playback device</button> : null}
        {error ?? route?.fault?.message ? <p className="notice error" role="alert">{error ?? route?.fault?.message}</p> : null}
        {source ? <section className="call-setup" aria-labelledby="speaker-setup-title">
          <AudioIcon kind="headphones" className="handoff-icon" />
          <div className="call-setup-content">
            <h3 id="speaker-setup-title">In your call app, choose this speaker:</h3>
            <p className="call-device-name">{source.name}</p>
            <details className="setup-help"><summary>How to connect</summary><ol>
              <li>Choose where to listen above, then turn on OpenKrisp.</li>
              <li>Open the audio settings in your call app.</li>
              <li>Select <strong>{source.name}</strong> as its speaker.</li>
            </ol><p>Headphones help prevent echo. Use your usual headphones or speakers for music.</p>
            <p>Before turning off OpenKrisp, switch your call app back to your usual headphones or speakers.</p></details>
          </div>
        </section> : null}
        {hasStarted ? <p className="hint call-audio-reminder">Before turning off OpenKrisp, switch your call app back to your usual headphones or speakers.</p> : null}
      </div>
    </section>
    <section className="settings-section" aria-labelledby="incoming-title" hidden={!helpOpen || (!hasStarted && !error && !route?.fault)}>
      <div className="panel-heading"><h2 id="incoming-title">Your speakers</h2>
        {hasStarted ? <button disabled={locked || !routeCanStop(route, pending !== null)} onClick={stop}>{route?.state === "stopping" ? "Turning off OpenKrisp…" : "Turn off OpenKrisp"}</button> : null}</div>
      {hasStarted ? <p className="settings-playback-status" role="status">{audioStatus(route, connected, pending !== null)}. Switch your call app back to your usual speakers before turning off OpenKrisp.</p> : null}
      {error ?? route?.fault?.message ? <p className="notice error" role="alert">{error ?? route?.fault?.message}</p> : null}
    </section>
  </>;
}
