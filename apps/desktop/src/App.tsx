import { useEffect, useReducer, useRef, useState } from "react";

import { api, subscribeToEngineEvents, type ApiError } from "./api";
import { connectEngine } from "./connection";
import { EndpointSetup } from "./EndpointSetup";
import { isPipeWireDefaultAlias, routeCanStop, routeIsActive, routeSelectionError, routeToStop, selectionFromRoute, startRequest, type DestinationKind, type RouteSelection } from "./routing";
import { initialDesktopState, normalizeApiError, reduceDesktopState } from "./state";

export function App() {
  const [state, dispatch] = useReducer(reduceDesktopState, initialDesktopState);
  const [selection, setSelection] = useState<RouteSelection>({ inputId: "", outputId: "", destinationKind: "virtual_microphone" });
  const [commandError, setCommandError] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [pendingStart, setPendingStart] = useState<number | null>(null);
  const restoredRoute = useRef("");

  useEffect(() => connectEngine({
    getSnapshot: api.getEngineSnapshot,
    subscribe: subscribeToEngineEvents,
    onSnapshot: (snapshot) => dispatch({ type: "reconcile", snapshot }),
    onError: (error) => dispatch({ type: "connection_failed", error: normalizeApiError(error) }),
    focusTarget: window,
  }), []);

  const snapshot = state.snapshot;
  const route = snapshot?.routes[0];
  const active = routeIsActive(route);
  // Restore the authoritative route after reload or an externally started route. Metrics do not
  // overwrite an idle user's draft selection, and disappeared devices never select a fallback.
  useEffect(() => {
    if (!route) return;
    const key = JSON.stringify([route.config.id, route.config.source, route.config.destination]);
    if (restoredRoute.current !== key) {
      restoredRoute.current = key;
      setSelection(selectionFromRoute(route));
    }
  }, [route]);
  useEffect(() => {
    if (pendingStart !== null && snapshot && snapshot.revision > pendingStart &&
      (routeIsActive(snapshot.routes[0]) || snapshot.lastFault !== null)) setPendingStart(null);
  }, [snapshot, pendingStart]);

  const inputs = snapshot?.inventory.devices.filter((device) => device.supportsInput && !isPipeWireDefaultAlias(device.id) &&
    (selection.destinationKind === "virtual_microphone" ? device.kind === "physical" : device.kind !== "virtual")) ?? [];
  const outputs = snapshot?.inventory.devices.filter((device) => device.supportsOutput && !isPipeWireDefaultAlias(device.id) && device.kind !== "virtual") ?? [];
  const selectionError = snapshot ? routeSelectionError(snapshot, selection) : "Connecting to the audio engine.";
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
    void runCommand(() => api.startRoute(startRequest(selection)));
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
      <fieldset disabled={controlsLocked || active || pendingStart !== null}>
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
      {!active && selectionError ? <p className="hint">{selectionError}</p> : null}
      {selection.destinationKind === "physical_output" ? <p className="hint">Use headphones to avoid acoustic feedback.</p> : null}
      <div className="actions">
        <button className="primary" disabled={controlsLocked || active || pendingStart !== null || selectionError !== null} onClick={start}>Start microphone</button>
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
    {snapshot ? <EndpointSetup routing={snapshot.routing} /> : null}
    {route ? <details className="panel">
      <summary>Route diagnostics</summary>
      <dl className="facts">
        <div><dt>Input</dt><dd>{route.inputFormat ? `${route.inputFormat.sampleRateHz} Hz / ${route.inputFormat.channels} ch` : "—"}</dd></div>
        <div><dt>Output</dt><dd>{route.outputFormat ? `${route.outputFormat.sampleRateHz} Hz / ${route.outputFormat.channels} ch` : "—"}</dd></div>
        <div><dt>Latency</dt><dd>{route.metrics.estimatedLatencyMs.toFixed(1)} ms</dd></div>
        <div><dt>Inference p99</dt><dd>{(route.metrics.inferenceP99Us / 1000).toFixed(1)} ms</dd></div>
        <div><dt>Input overruns</dt><dd>{route.metrics.inputOverruns}</dd></div>
        <div><dt>Output underruns</dt><dd>{route.metrics.outputUnderruns}</dd></div>
        <div><dt>Drift</dt><dd>{route.metrics.driftCorrectionPpm.toFixed(0)} ppm</dd></div>
        <div><dt>Recoveries</dt><dd>{route.successfulRecoveries} / {route.recoveryAttempts}</dd></div>
      </dl>
    </details> : null}
  </main>;
}
