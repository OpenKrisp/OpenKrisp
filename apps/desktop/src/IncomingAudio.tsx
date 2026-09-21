import { useEffect, useRef, useState } from "react";
import { api, type EngineSnapshot, type ProcessingMode } from "./api";
import { incomingRouteId, incomingSelectionError } from "./incoming";
import { isPipeWireDefaultAlias, routeCanStop, routeIsActive } from "./routing";
import { normalizeApiError } from "./state";
import { DenoiseControls } from "./DenoiseControls";
import { EchoCancellation } from "./EchoCancellation";
import { AudioIcon, AudioLevel, audioStatus, NoiseReductionSwitch } from "./AudioControls";
import { draftSettingsError, requestFromDraft, type RouteDraft } from "./drafts";

export function IncomingAudio({ snapshot, connected, draft, onDraft, settingsReady, settingsOpen = false }: {
  snapshot: EngineSnapshot; connected: boolean; draft: RouteDraft; onDraft: (draft: RouteDraft) => void;
  settingsReady: boolean; settingsOpen?: boolean;
}) {
  const { inputId: sourceId, outputId, mode } = draft;
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [pending, setPending] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const route = snapshot.routes.find((route) => !("physicalInput" in route.config.source));
  const microphone = snapshot.routes.find((route) => "physicalInput" in route.config.source);
  const active = routeIsActive(route);
  const [expanded, setExpanded] = useState(active);
  const source = snapshot.routing.loopbackEndpoints?.find((endpoint) => endpoint.id === sourceId);
  const outputs = snapshot.inventory.devices.filter((device) => device.supportsOutput && device.kind === "physical" && !isPipeWireDefaultAlias(device.id));
  const validation = incomingSelectionError(snapshot, sourceId, outputId) ?? draftSettingsError(draft);
  const locked = busy || !connected || snapshot.shuttingDown;
  const hasStarted = active || pending !== null;
  const editingLocked = locked || !settingsReady || hasStarted;

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
    <details className="incoming-panel" hidden={settingsOpen} open={expanded} onToggle={(event) => setExpanded(event.currentTarget.open)}>
      <summary><AudioIcon kind="headphones" /><span className="incoming-summary"><span>Call audio</span><span>Clean up the voices you hear</span></span>
        <span className={`status ${connected && route?.state === "running" ? "on" : ""}`} role="status">{hasStarted || route?.state === "failed" || !connected ? audioStatus(route, connected, pending !== null) : "Optional"}</span><span className="chevron" aria-hidden="true" /></summary>
      <section className="incoming-content" aria-label="Call audio controls">
        <p className="section-description">Listen to calls through OpenKrisp. Noise reduction is optional and works best for speech.</p>
        <fieldset disabled={editingLocked}>
          <label>Play through<select value={outputId} onChange={(event) => onDraft({ ...draft, outputId: event.target.value })}>
            <option value="">Choose headphones or speakers</option>
            {outputId && !outputs.some((output) => output.id === outputId) ? <option value={outputId}>Saved headphones or speakers unavailable</option> : null}
            {outputs.map((output) => <option key={output.id} value={output.id}>{output.name}</option>)}
          </select></label>
        </fieldset>
        {hasStarted ? <p className="device-hint">Stop call audio to change devices.</p> : null}
        <NoiseReductionSwitch label="Call audio noise reduction" draft={draft} onDraft={onDraft} route={route} locked={locked || !settingsReady || pending !== null} />
        {!active && validation ? <p className="notice">{validation}</p> : null}
        <button className="secondary transport" disabled={locked || (hasStarted ? !routeCanStop(route, pending !== null) : !settingsReady || validation !== null)} onClick={hasStarted ? stop : () => {
          setPending(snapshot.revision); void run(() => api.startRoute(requestFromDraft(draft)));
        }}>{hasStarted ? route?.state === "stopping" ? "Stopping call audio…" : pending !== null && !active ? "Cancel call audio start" : "Stop call audio" : "Start call audio"}</button>
        {route?.state === "failed" ? <button className="text-button cleanup-button" disabled={locked} onClick={stop}>Release playback device</button> : null}
        {error ?? route?.fault?.message ? <p className="notice error" role="alert">{error ?? route?.fault?.message}</p> : null}
        {hasStarted ? <AudioLevel label="Playback level" value={route?.metrics.outputPeak ?? 0} active={connected && route?.state === "running"} /> : null}
        {source ? <div className="incoming-handoff"><p>In your call app’s audio settings, choose:</p><div className="handoff-device"><span>Speaker</span><strong>{source.name}</strong></div><p>Stop call audio only after switching your call app back to your usual speakers.</p></div> : null}
        <p className="hint">Headphones work best. For speakers, check echo cancellation below.</p>
        <EchoCancellation route={microphone} enabled={microphone?.config.echoCancellation ?? true} connected={connected} snapshot={snapshot} />
      </section>
    </details>
    <section className="settings-section" aria-labelledby="incoming-title" hidden={!settingsOpen}>
      <div className="panel-heading"><h2 id="incoming-title">Call audio</h2><button className="quiet-button" disabled={locked} onClick={() => void run(api.refreshAudioDevices)}>Refresh devices</button></div>
      {hasStarted ? <div className="settings-running"><p>Stop call audio to change devices.</p><button disabled={locked || !routeCanStop(route, pending !== null)} onClick={stop}>Stop call audio</button></div> : null}
      <fieldset disabled={editingLocked}>
        <label>Audio from<select value={sourceId} onChange={(event) => onDraft({ ...draft, inputId: event.target.value })}>
          <option value="">Choose a call audio device</option>
          {sourceId && !source ? <option value={sourceId}>Saved call audio device unavailable</option> : null}
          {snapshot.routing.loopbackEndpoints?.map((endpoint) => <option key={endpoint.id} value={endpoint.id}>{endpoint.name}</option>)}
        </select></label>
        <label>Processing<select value={mode} onChange={(event) => onDraft({ ...draft, mode: event.target.value as ProcessingMode })}>
          <option value="aligned_bypass">Original sound (music and general audio)</option><option value="speech_denoise">Noise reduction for speech</option>
        </select></label>
      </fieldset>
      <DenoiseControls label="Call audio" settings={draft.denoise} onChange={(denoise) => onDraft({ ...draft, denoise })}
        route={route} locked={locked || !settingsReady || pending !== null}
        startOptions={<fieldset disabled={editingLocked}><label>Incoming target latency (ms)<input type="number" min={1} max={1000} step={1} value={Number.isNaN(draft.targetLatencyMs) ? "" : draft.targetLatencyMs}
          onChange={(event) => onDraft({ ...draft, targetLatencyMs: event.target.value === "" ? NaN : Number(event.target.value) })} /></label></fieldset>} />
      {source && source.instructions.length > 0 ? <details className="setup-help"><summary>Call audio setup help</summary><ol>{source.instructions.map((instruction) => <li key={instruction}>{instruction}</li>)}</ol></details> : null}
      {error ? <p className="notice error" role="alert">{error}</p> : null}
    </section>
  </>;
}
