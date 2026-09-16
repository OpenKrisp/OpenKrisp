import { useEffect, useRef, useState } from "react";
import { api, type EngineSnapshot, type ProcessingMode } from "./api";
import { incomingRouteId, incomingSelectionError } from "./incoming";
import { isPipeWireDefaultAlias, routeCanStop, routeIsActive } from "./routing";
import { normalizeApiError } from "./state";
import { DenoiseControls } from "./DenoiseControls";
import { draftSettingsError, requestFromDraft, type RouteDraft } from "./drafts";

export function IncomingAudio({ snapshot, connected, draft, onDraft, profilesBusy, onPending }: {
  snapshot: EngineSnapshot; connected: boolean; draft: RouteDraft; onDraft: (draft: RouteDraft) => void;
  profilesBusy: boolean; onPending: (pending: boolean) => void;
}) {
  const { inputId: sourceId, outputId, mode } = draft;
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [pending, setPending] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const route = snapshot.routes.find((route) => !("physicalInput" in route.config.source));
  const active = routeIsActive(route);
  const source = snapshot.routing.loopbackEndpoints?.find((endpoint) => endpoint.id === sourceId);
  const outputs = snapshot.inventory.devices.filter((device) => device.supportsOutput && device.kind === "physical" && !isPipeWireDefaultAlias(device.id));
  const validation = incomingSelectionError(snapshot, sourceId, outputId) ?? draftSettingsError(draft);
  const locked = busy || !connected || snapshot.shuttingDown;

  useEffect(() => { onPending(pending !== null || busy); }, [pending, busy, onPending]);
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

  return <section className="panel" aria-labelledby="incoming-title">
    <div className="panel-heading"><h2 id="incoming-title">Incoming audio</h2>
      <button disabled={locked} onClick={() => void run(api.refreshAudioDevices)}>Refresh incoming devices</button>
    </div>
    <p>Play through headphones or speakers. For speakers, check that microphone echo cancellation is active. Enable noise reduction for speech; keep music and other audio in bypass.</p>
    <p role="status">Route: {route?.state ?? "stopped"}{pending !== null ? " · start requested" : ""}</p>
    <fieldset disabled={locked || profilesBusy || active || pending !== null}>
      <label>Audio from
        <select value={sourceId} onChange={(event) => onDraft({ ...draft, inputId: event.target.value })}>
          <option value="">Choose an incoming endpoint</option>
          {sourceId && !source ? <option value={sourceId}>Selected endpoint unavailable</option> : null}
          {snapshot.routing.loopbackEndpoints?.map((endpoint) => <option key={endpoint.id} value={endpoint.id}>{endpoint.name} · {endpoint.availability}</option>)}
        </select>
      </label>
      <label>Play through
        <select value={outputId} onChange={(event) => onDraft({ ...draft, outputId: event.target.value })}>
          <option value="">Choose headphones or speakers</option>
          {outputId && !outputs.some((output) => output.id === outputId) ? <option value={outputId}>Selected output unavailable</option> : null}
          {outputs.map((output) => <option key={output.id} value={output.id}>{output.name}</option>)}
        </select>
      </label>
      <label>Processing
        <select value={mode} onChange={(event) => onDraft({ ...draft, mode: event.target.value as ProcessingMode })}>
          <option value="aligned_bypass">Bypass · music and general audio</option>
          <option value="speech_denoise">Speech noise reduction</option>
        </select>
      </label>
    </fieldset>
    <DenoiseControls label="Incoming" settings={draft.denoise} onChange={(denoise) => onDraft({ ...draft, denoise })}
      route={route} locked={locked || profilesBusy || pending !== null}
      startOptions={<fieldset disabled={locked || profilesBusy || active || pending !== null}>
      <label>Incoming target latency (ms)<input type="number" min={1} max={1000} step={1} value={Number.isNaN(draft.targetLatencyMs) ? "" : draft.targetLatencyMs}
        onChange={(event) => onDraft({ ...draft, targetLatencyMs: event.target.value === "" ? NaN : Number(event.target.value) })} /></label>
      </fieldset>} />
    {source ? <ol>{source.instructions.map((instruction) => <li key={instruction}>{instruction}</li>)}</ol> : null}
    {!active && validation ? <p className="hint">{validation}</p> : null}
    <div className="actions">
      <button className="primary" disabled={locked || profilesBusy || active || pending !== null || validation !== null} onClick={() => {
        setPending(snapshot.revision); void run(() => api.startRoute(requestFromDraft(draft)));
      }}>Start incoming audio</button>
      <button disabled={locked || !routeCanStop(route, pending !== null)} onClick={() => void run(async () => {
        await api.stopRoute(pending !== null ? incomingRouteId : route?.config.id ?? incomingRouteId); setPending(null);
      })}>Stop incoming audio</button>
      <button disabled={locked || route?.state !== "running"} aria-pressed={route ? !route.bypassed : false} onClick={() => route && void run(() => api.setRouteBypass(route.config.id, !route.bypassed))}>
        {route?.bypassed ? "Enable speech noise reduction" : "Bypass incoming noise reduction"}
      </button>
    </div>
    {route?.metrics.degraded ? <p role="status">Incoming noise reduction is temporarily bypassed to keep audio flowing and prioritize the microphone.</p> : null}
    {error ?? route?.fault?.message ? <p role="alert">{error ?? route?.fault?.message}</p> : null}
    <label>Incoming output level<meter min={0} max={1} value={active ? route?.metrics.outputPeak ?? 0 : 0} /></label>
    {route ? <p className="hint">{route.inputFormat?.channels ?? "—"} channels · {route.metrics.estimatedLatencyMs.toFixed(1)} ms estimated latency · inference p99 {(route.metrics.inferenceP99Us / 1000).toFixed(1)} ms</p> : null}
  </section>;
}
