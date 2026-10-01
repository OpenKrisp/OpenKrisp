import { useEffect, useReducer, useRef, useState } from "react";

import { api, subscribeToEngineEvents, type ApiError } from "./api";
import { connectEngine } from "./connection";
import { IncomingAudio } from "./IncomingAudio";
import { EndpointSetup } from "./EndpointSetup";
import { SupportDetails } from "./SupportDetails";
import { automaticDraft } from "./preferences";
import { AudioIcon, AudioLevel, audioStatus } from "./AudioControls";
import { MicrophoneFlow } from "./MicrophoneFlow";
import { useSettings } from "./useSettings";
import { draftSettingsError, requestFromDraft } from "./drafts";
import { useRouteDraft } from "./useRouteDraft";
import { isPipeWireDefaultAlias, routeCanStop, routeIsActive, routeSelectionError, routeToStop } from "./routing";
import { initialDesktopState, normalizeApiError, reduceDesktopState } from "./state";

export function App() {
  const [state, dispatch] = useReducer(reduceDesktopState, initialDesktopState);
  const [commandError, setCommandError] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [pendingStart, setPendingStart] = useState<number | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const helpHeading = useRef<HTMLHeadingElement>(null);
  const helpButton = useRef<HTMLButtonElement>(null);
  const navigationRequested = useRef(false);

  useEffect(() => connectEngine({
    getSnapshot: api.getEngineSnapshot,
    subscribe: subscribeToEngineEvents,
    onSnapshot: (snapshot) => dispatch({ type: "reconcile", snapshot }),
    onError: (error) => dispatch({ type: "connection_failed", error: normalizeApiError(error) }),
    focusTarget: window,
  }), []);

  useEffect(() => {
    if (!navigationRequested.current) return;
    (helpOpen ? helpHeading.current : helpButton.current)?.focus();
    window.scrollTo?.(0, 0);
  }, [helpOpen]);

  const snapshot = state.snapshot;
  const settings = useSettings(snapshot);
  const route = snapshot?.routes.find((route) => "physicalInput" in route.config.source);
  const active = routeIsActive(route);
  const [draft, setSelection] = useRouteDraft("microphone", route, settings.initial?.microphone, settings.edit);
  const selection = snapshot && !active && pendingStart === null ? automaticDraft(draft, snapshot) : draft;
  const startSelection: typeof selection = { ...selection, mode: "speech_denoise", denoise: { ...selection.denoise, enabled: true } };
  const incomingRoute = snapshot?.routes.find((route) => !("physicalInput" in route.config.source));
  const [incomingDraft, setIncomingDraft] = useRouteDraft("incoming", incomingRoute, settings.initial?.incoming, settings.edit);
  useEffect(() => {
    if (pendingStart !== null && snapshot && snapshot.revision > pendingStart &&
      (routeIsActive(snapshot.routes.find((route) => "physicalInput" in route.config.source)) || snapshot.lastFault !== null)) setPendingStart(null);
  }, [snapshot, pendingStart]);

  const inputs = snapshot?.inventory.devices.filter((device) => device.supportsInput && !isPipeWireDefaultAlias(device.id) &&
    (selection.destinationKind === "virtual_microphone" ? device.kind === "physical" : device.kind !== "virtual")) ?? [];
  const endpoint = snapshot?.routing.endpoints.find((endpoint) => endpoint.id === selection.outputId);
  const selectionError = snapshot ? (routeSelectionError(snapshot, selection) ?? draftSettingsError(selection)) : null;
  const fault = commandError ?? state.fault ?? route?.fault;
  const controlsLocked = busy || !state.connected || snapshot?.shuttingDown === true;
  const editingLocked = controlsLocked || !settings.ready || active || pendingStart !== null;
  const hasStarted = active || pendingStart !== null;
  const starting = route?.state === "starting" || (pendingStart !== null && !active);
  const actionLabel = !hasStarted ? "Turn on OpenKrisp" : route?.state === "stopping" ? "Turning off OpenKrisp…"
    : starting ? "Cancel start" : "Turn off OpenKrisp";
  const running = state.connected && route?.state === "running";
  const status = audioStatus(route, state.connected, pendingStart !== null);
  const settingsProblem = Boolean(settings.error || settings.status?.writeError || settings.status?.loadWarning);

  function navigate(open: boolean) {
    navigationRequested.current = true;
    setHelpOpen(open);
  }

  async function runCommand(action: () => Promise<unknown>) {
    if (busyRef.current) return;
    busyRef.current = true; setBusy(true); setCommandError(null);
    try { await action(); }
    catch (error) { setCommandError(normalizeApiError(error)); setPendingStart(null); }
    finally { busyRef.current = false; setBusy(false); }
  }

  function start() {
    if (!snapshot || selectionError || active || pendingStart !== null) return;
    setSelection(startSelection);
    setPendingStart(snapshot.revision);
    void runCommand(() => api.startRoute(requestFromDraft(startSelection)));
  }

  function stop() {
    const routeId = routeToStop(route, pendingStart !== null);
    if (!routeId) return;
    void runCommand(async () => { await api.stopRoute(routeId); setPendingStart(null); });
  }

  return <main className={`app-shell ${helpOpen ? "is-help" : ""}`} onKeyDown={(event) => { if (event.key === "Escape" && helpOpen) navigate(false); }}>
    <header className="app-header">
      <div className="wordmark"><span className="brand-mark" aria-hidden="true"><i /><i /><i /></span>OpenKrisp</div>
      {helpOpen ? <button className="quiet-button" onClick={() => navigate(false)}><AudioIcon kind="back" />Back</button>
        : <button ref={helpButton} className="quiet-button" onClick={() => navigate(true)}><AudioIcon kind="help" />Help</button>}
    </header>

    {fault ? <aside className="notice error" role="alert"><strong>{fault.message}</strong>{fault.suggestedAction ? <span>{fault.suggestedAction}</span> : null}</aside> : null}
    {settingsProblem ? <aside className="notice error" aria-label="Settings persistence">
      {settings.status?.loadWarning ? <><p role="alert">{settings.status.loadWarning.message} Your choices can be used for this session.</p>
        <button disabled={!settings.ready || settings.saving || snapshot?.shuttingDown} onClick={() => void settings.retry(true)}>Replace unreadable settings</button></> : null}
      {settings.error ?? settings.status?.writeError?.message ? <><p role="alert">{settings.error ?? settings.status?.writeError?.message}</p>
        <button disabled={settings.loading || snapshot?.shuttingDown} onClick={() => void settings.retry()}>Retry settings</button></> : null}
    </aside> : null}

    <h1 className="sr-only" hidden={helpOpen}>Noise reduction</h1>
    <div className="help-heading" hidden={!helpOpen}><h1 ref={helpHeading} tabIndex={-1}>Help</h1></div>
    <div className={helpOpen ? "help-audio-status" : "audio-workspace"}>
      <section className="audio-panel microphone-panel" aria-labelledby="microphone-title" hidden={helpOpen}>
        <div className="audio-panel-heading">
          <h2 id="microphone-title">Your microphone</h2>
          <span className={`status ${running ? "on" : ""}`} role="status"><span className={starting && state.connected ? "status-spinner" : "status-dot"} aria-hidden="true" />{status}</span>
        </div>
        <p className="audio-panel-description">Reduce background noise around you.</p>
        <MicrophoneFlow draft={active ? selection : startSelection} route={route} running={running} waiting={!state.connected || !settings.ready || (hasStarted && !running)} />
        <div className="device-heading"><label htmlFor="microphone-device">Microphone</label></div>
        <select id="microphone-device" aria-describedby={hasStarted ? "microphone-locked" : undefined} disabled={editingLocked} value={selection.inputId} onChange={(event) => setSelection({ ...selection, inputId: event.target.value })}>
          <option value="">Choose a microphone</option>
          {selection.inputId && !inputs.some((device) => device.id === selection.inputId) ? <option value={selection.inputId}>Saved microphone unavailable</option> : null}
          {inputs.map((device) => <option key={device.id} value={device.id}>{device.name}{device.isDefaultInput ? " (default)" : ""}</option>)}
        </select>
        {hasStarted ? <p id="microphone-locked" className="device-hint">{starting && state.connected ? "Preparing noise reduction. This can take a few seconds." : "Turn off OpenKrisp to change devices."}</p> : null}
        <AudioLevel label="Input level" value={route?.metrics.inputPeak ?? 0} active={running} />
        {running && route?.metrics.degraded ? <p className="notice" role="status">Noise reduction is temporarily paused to keep your audio playing.</p> : null}
        {!settings.ready && !settingsProblem ? <p role="status" className="hint">{settings.loading ? "Loading saved settings…" : "Waiting for your audio devices…"}</p> : null}
        {!active && settings.ready && selectionError ? <div className="notice"><p>{selectionError}</p><button className="text-button" onClick={() => navigate(true)}>Get setup help</button></div> : null}
        {selection.destinationKind === "physical_output" ? <p className="notice">You’re in microphone test mode. Use headphones to avoid feedback. Turn off OpenKrisp to return to call mode on your next start.</p> : null}
        <button className={`primary transport ${hasStarted ? "transport-stop" : ""}`}
          disabled={controlsLocked || (hasStarted ? !routeCanStop(route, pendingStart !== null) : !settings.ready || selectionError !== null)}
          onClick={hasStarted ? stop : start}>
          <span className={hasStarted ? "stop-icon" : "start-icon"} aria-hidden="true" />
          {actionLabel}
        </button>
        {route?.state === "failed" ? <button className="text-button cleanup-button" disabled={controlsLocked} onClick={stop}>Release audio device</button> : null}
      {selection.destinationKind === "virtual_microphone" ? <section className="call-setup" aria-labelledby="call-setup-title">
        <AudioIcon kind="microphone" className="handoff-icon" />
        <div className="call-setup-content">
          <h3 id="call-setup-title">In your call app, choose this microphone:</h3>
          <p className="call-device-name">{endpoint?.name ?? "OpenKrisp Microphone"}</p>
          <details className="setup-help"><summary>How to connect</summary><ol>
            <li>Turn on OpenKrisp.</li>
            <li>Open the audio settings in your calling or recording app.</li>
            <li>Select <strong>{endpoint?.name ?? "the OpenKrisp microphone"}</strong> as its microphone.</li>
          </ol><p>Keep OpenKrisp running during the call. If the device is missing, open Help.</p></details>
        </div>
      </section> : null}
      </section>

      {hasStarted ? <section className="settings-running" aria-label="Microphone status" hidden={!helpOpen}><span className={`status ${running ? "on" : ""}`} role="status"><span className={starting && state.connected ? "status-spinner" : "status-dot"} aria-hidden="true" />Microphone: {status}</span>
        <button disabled={controlsLocked || !routeCanStop(route, pendingStart !== null)} onClick={stop}>{actionLabel}</button></section> : null}

      {snapshot ? <IncomingAudio snapshot={snapshot} connected={state.connected} draft={incomingDraft} onDraft={setIncomingDraft} settingsReady={settings.ready}
        helpOpen={helpOpen} onOpenHelp={() => navigate(true)} /> : <section className="audio-panel" hidden={helpOpen} aria-labelledby="incoming-loading-title">
          <div className="audio-panel-heading"><h2 id="incoming-loading-title">Your speakers</h2></div>
          <p className="audio-panel-description">Reduce background noise from other people in your call.</p>
          <p role="status">Waiting for your audio devices…</p>
        </section>}
    </div>

    <div hidden={!helpOpen}>
      {snapshot ? <EndpointSetup routing={snapshot.routing} /> : <p className="connection-guide" role="status">Waiting for your audio devices…</p>}
      <section className="help-troubleshooting" aria-labelledby="troubleshooting-title">
        <h2 id="troubleshooting-title">Troubleshooting</h2>
        <section className="help-action" aria-labelledby="refresh-title">
          <div className="help-action-description"><h3 id="refresh-title">Missing a device?</h3><p>Reconnect it, then refresh the list.</p></div>
          <button className="secondary" disabled={controlsLocked} onClick={() => void runCommand(api.refreshAudioDevices)}>Refresh devices</button>
        </section>
        {snapshot ? <SupportDetails snapshot={snapshot} connected={state.connected} /> : null}
      </section>
    </div>
    <footer className="app-footer" hidden={helpOpen}><AudioIcon kind="shield" /><span>Private by design. Audio is processed on your device.</span></footer>
  </main>;
}
