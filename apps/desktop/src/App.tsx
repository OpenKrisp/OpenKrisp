import { useEffect, useReducer, useRef, useState } from "react";

import { api, subscribeToEngineEvents, type ApiError } from "./api";
import { connectEngine } from "./connection";
import { IncomingAudio } from "./IncomingAudio";
import { EndpointSetup } from "./EndpointSetup";
import { DenoiseControls } from "./DenoiseControls";
import { DiagnosticPanel } from "./DiagnosticPanel";
import { Profiles } from "./Profiles";
import { draftSettingsError, requestFromDraft } from "./drafts";
import { useRouteDraft } from "./useRouteDraft";
import { isPipeWireDefaultAlias, routeCanStop, routeIsActive, routeSelectionError, routeToStop, type DestinationKind } from "./routing";
import { initialDesktopState, normalizeApiError, reduceDesktopState } from "./state";

export function App() {
  const [state, dispatch] = useReducer(reduceDesktopState, initialDesktopState);
  const [commandError, setCommandError] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [pendingStart, setPendingStart] = useState<number | null>(null);
  const [profilesBusy, setProfilesBusy] = useState(false);
  const [incomingPending, setIncomingPending] = useState(false);

  useEffect(() => connectEngine({
    getSnapshot: api.getEngineSnapshot,
    subscribe: subscribeToEngineEvents,
    onSnapshot: (snapshot) => dispatch({ type: "reconcile", snapshot }),
    onError: (error) => dispatch({ type: "connection_failed", error: normalizeApiError(error) }),
    focusTarget: window,
  }), []);

  const snapshot = state.snapshot;
  const route = snapshot?.routes.find((route) => "physicalInput" in route.config.source);
  const active = routeIsActive(route);
  const [selection, setSelection] = useRouteDraft("microphone", route);
  const incomingRoute = snapshot?.routes.find((route) => !("physicalInput" in route.config.source));
  const [incomingDraft, setIncomingDraft] = useRouteDraft("incoming", incomingRoute);
  useEffect(() => {
    if (pendingStart !== null && snapshot && snapshot.revision > pendingStart &&
      (routeIsActive(snapshot.routes.find((route) => "physicalInput" in route.config.source)) || snapshot.lastFault !== null)) setPendingStart(null);
  }, [snapshot, pendingStart]);

  const inputs = snapshot?.inventory.devices.filter((device) => device.supportsInput && !isPipeWireDefaultAlias(device.id) &&
    (selection.destinationKind === "virtual_microphone" ? device.kind === "physical" : device.kind !== "virtual")) ?? [];
  const outputs = snapshot?.inventory.devices.filter((device) => device.supportsOutput && !isPipeWireDefaultAlias(device.id) && device.kind !== "virtual") ?? [];
  const selectionError = snapshot ? (routeSelectionError(snapshot, selection) ?? draftSettingsError(selection)) : "Connecting to the audio engine.";
  const fault = commandError ?? state.fault ?? route?.fault;
  const controlsLocked = busy || !state.connected || snapshot?.shuttingDown === true;

  async function runCommand(action: () => Promise<unknown>) {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setCommandError(null);
    try { await action(); }
    catch (error) { setCommandError(normalizeApiError(error)); setPendingStart(null); }
    finally { busyRef.current = false; setBusy(false); }
  }

  function start() {
    if (!snapshot || selectionError || active || pendingStart !== null) return;
    setPendingStart(snapshot.revision);
    void runCommand(() => api.startRoute(requestFromDraft(selection)));
  }

  function stop() {
    const routeId = routeToStop(route, pendingStart !== null);
    if (!routeId) return;
    void runCommand(async () => { await api.stopRoute(routeId); setPendingStart(null); });
  }

  return <main>
    <div className="eyebrow">OPENKRISP / MICROPHONE</div>
    <section className="card">
      <div className={`status-dot ${state.connected ? "online" : ""}`} aria-hidden="true" />
      <div>
        <h1>A clearer microphone</h1>
        <p>Clean up background noise and send your voice to a call or recording app.</p>
        <p role="status">{state.connected ? `Route: ${route?.state ?? "stopped"}${pendingStart !== null && !active ? " · start requested" : ""}` : "Connecting to audio engine…"}</p>
      </div>
    </section>
    <section className="panel" aria-labelledby="route-title">
      <div className="panel-heading"><h2 id="route-title">Microphone route</h2>
        <button disabled={controlsLocked} onClick={() => void runCommand(api.refreshAudioDevices)}>Refresh devices</button>
      </div>
      <fieldset disabled={controlsLocked || profilesBusy || active || pendingStart !== null}>
        <label>Send audio to
          <select value={selection.destinationKind} onChange={(event) => setSelection({ ...selection, destinationKind: event.target.value as DestinationKind, outputId: "" })}>
            <option value="virtual_microphone" disabled={!snapshot?.routing.capabilities.virtualMicrophone}>Virtual microphone{snapshot && !snapshot.routing.capabilities.virtualMicrophone ? " (unavailable)" : ""}</option>
            <option value="physical_output">Headphone monitor</option>
          </select>
        </label>
        <label>Microphone
          <select value={selection.inputId} onChange={(event) => setSelection({ ...selection, inputId: event.target.value })}>
            <option value="">Choose a microphone</option>
            {selection.inputId && !inputs.some((device) => device.id === selection.inputId) ? <option value={selection.inputId}>Selected microphone unavailable or unsupported</option> : null}
            {inputs.map((device) => <option key={device.id} value={device.id}>{device.name}{device.isDefaultInput ? " (system default)" : ""}</option>)}
          </select>
        </label>
        <label>{selection.destinationKind === "virtual_microphone" ? "Virtual endpoint" : "Headphones"}
          <select value={selection.outputId} onChange={(event) => setSelection({ ...selection, outputId: event.target.value })}>
            <option value="">Choose an output</option>
            {selection.destinationKind === "virtual_microphone" ? <>
              {selection.outputId && !snapshot?.routing.endpoints.some((endpoint) => endpoint.id === selection.outputId) ? <option value={selection.outputId}>Selected endpoint unavailable</option> : null}
              {snapshot?.routing.endpoints.map((endpoint) => <option key={endpoint.id} value={endpoint.id}>{endpoint.name} · {endpoint.availability}</option>)}
            </> : <>
              {selection.outputId && !outputs.some((device) => device.id === selection.outputId) ? <option value={selection.outputId}>Selected output unavailable</option> : null}
              {outputs.map((device) => <option key={device.id} value={device.id}>{device.name}{device.isDefaultOutput ? " (system default)" : ""}</option>)}
            </>}
          </select>
        </label>
      </fieldset>
      <DenoiseControls label="Microphone" settings={selection.denoise} onChange={(denoise) => setSelection({ ...selection, denoise })}
        route={route} locked={controlsLocked || profilesBusy || pendingStart !== null}
        startOptions={<fieldset disabled={controlsLocked || profilesBusy || active || pendingStart !== null}>
        <label>Microphone processing<select value={selection.mode} onChange={(event) => setSelection({ ...selection, mode: event.target.value as typeof selection.mode })}>
          <option value="speech_denoise">Speech noise reduction</option><option value="aligned_bypass">Bypass</option>
        </select></label>
        <label>Microphone target latency (ms)<input type="number" min={1} max={1000} step={1} value={Number.isNaN(selection.targetLatencyMs) ? "" : selection.targetLatencyMs}
          onChange={(event) => setSelection({ ...selection, targetLatencyMs: event.target.value === "" ? NaN : Number(event.target.value) })} /></label>
        </fieldset>} />
      {!active && selectionError ? <p className="hint">{selectionError}</p> : null}
      {selection.destinationKind === "physical_output" ? <p className="hint">Use headphones to avoid acoustic feedback.</p> : null}
      <div className="actions">
        <button className="primary" disabled={controlsLocked || profilesBusy || active || pendingStart !== null || selectionError !== null} onClick={start}>Start microphone</button>
        <button disabled={controlsLocked || !routeCanStop(route, pendingStart !== null)} onClick={stop}>Stop</button>
        <button disabled={controlsLocked || route?.state !== "running"} aria-pressed={route?.bypassed ?? false}
          onClick={() => route && void runCommand(() => api.setRouteBypass(route.config.id, !route.bypassed))}>
          {route?.bypassed ? "Enable noise reduction" : "Bypass noise reduction"}
        </button>
      </div>
      {route?.metrics.degraded ? <p role="status" className="hint">The engine is temporarily bypassing noise reduction to keep audio flowing.</p> : null}
    </section>
    {fault ? <aside role="alert"><strong>{fault.message}</strong>{fault.suggestedAction ? <span>{fault.suggestedAction}</span> : null}</aside> : null}
    <section className="panel meters" aria-label="Audio levels">
      <label>Microphone level<meter min={0} max={1} value={active ? route?.metrics.inputPeak ?? 0 : 0} /></label>
      <label>Processed output<meter min={0} max={1} value={active ? route?.metrics.outputPeak ?? 0 : 0} /></label>
    </section>
    {snapshot ? <IncomingAudio snapshot={snapshot} connected={state.connected} draft={incomingDraft} onDraft={setIncomingDraft} profilesBusy={profilesBusy} onPending={setIncomingPending} /> : null}
    {snapshot ? <EndpointSetup routing={snapshot.routing} /> : null}
    {snapshot ? <Profiles snapshot={snapshot} connected={state.connected} onBusy={setProfilesBusy}
      targets={{ microphone: { draft: selection, locked: active || pendingStart !== null || busy },
        incoming: { draft: incomingDraft, locked: routeIsActive(incomingRoute) || incomingPending } }}
      onLoad={(draft) => draft.kind === "microphone" ? setSelection(draft) : setIncomingDraft(draft)} /> : null}
    {snapshot ? <DiagnosticPanel snapshot={snapshot} connected={state.connected} /> : null}
  </main>;
}
