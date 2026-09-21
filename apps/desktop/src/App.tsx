import { useEffect, useReducer, useRef, useState } from "react";

import { api, subscribeToEngineEvents, type ApiError } from "./api";
import { connectEngine } from "./connection";
import { IncomingAudio } from "./IncomingAudio";
import { EndpointSetup } from "./EndpointSetup";
import { DenoiseControls } from "./DenoiseControls";
import { EchoCancellation } from "./EchoCancellation";
import { DiagnosticPanel } from "./DiagnosticPanel";
import { AudioIcon, AudioLevel, audioStatus, NoiseReductionSwitch } from "./AudioControls";
import { useSettings } from "./useSettings";
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
  const [settingsOpen, setSettingsOpen] = useState(false);
  const settingsHeading = useRef<HTMLHeadingElement>(null);
  const settingsButton = useRef<HTMLButtonElement>(null);
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
    (settingsOpen ? settingsHeading.current : settingsButton.current)?.focus();
    window.scrollTo?.(0, 0);
  }, [settingsOpen]);

  const snapshot = state.snapshot;
  const settings = useSettings(snapshot);
  const route = snapshot?.routes.find((route) => "physicalInput" in route.config.source);
  const active = routeIsActive(route);
  const [selection, setSelection] = useRouteDraft("microphone", route, settings.initial?.microphone, settings.edit);
  const incomingRoute = snapshot?.routes.find((route) => !("physicalInput" in route.config.source));
  const [incomingDraft, setIncomingDraft] = useRouteDraft("incoming", incomingRoute, settings.initial?.incoming, settings.edit);
  useEffect(() => {
    if (pendingStart !== null && snapshot && snapshot.revision > pendingStart &&
      (routeIsActive(snapshot.routes.find((route) => "physicalInput" in route.config.source)) || snapshot.lastFault !== null)) setPendingStart(null);
  }, [snapshot, pendingStart]);

  const inputs = snapshot?.inventory.devices.filter((device) => device.supportsInput && !isPipeWireDefaultAlias(device.id) &&
    (selection.destinationKind === "virtual_microphone" ? device.kind === "physical" : device.kind !== "virtual")) ?? [];
  const outputs = snapshot?.inventory.devices.filter((device) => device.supportsOutput && !isPipeWireDefaultAlias(device.id) && device.kind !== "virtual") ?? [];
  const endpoint = snapshot?.routing.endpoints.find((endpoint) => endpoint.id === selection.outputId);
  const selectionError = snapshot ? (routeSelectionError(snapshot, selection) ?? draftSettingsError(selection)) : null;
  const fault = commandError ?? state.fault ?? route?.fault;
  const controlsLocked = busy || !state.connected || snapshot?.shuttingDown === true;
  const editingLocked = controlsLocked || !settings.ready || active || pendingStart !== null;
  const hasStarted = active || pendingStart !== null;
  const running = state.connected && route?.state === "running";
  const status = audioStatus(route, state.connected, pendingStart !== null);
  const title = !state.connected ? snapshot ? "Reconnecting to audio" : "Getting ready…"
    : snapshot?.shuttingDown ? "Closing OpenKrisp…"
    : running ? "Your microphone is on" : hasStarted ? status : route?.state === "failed" ? "Let’s get you reconnected"
    : settings.ready && selectionError ? "Check your audio setup" : "Ready when you are";
  const settingsProblem = Boolean(settings.error || settings.status?.writeError || settings.status?.loadWarning);

  function navigate(open: boolean) { navigationRequested.current = true; setSettingsOpen(open); }

  async function runCommand(action: () => Promise<unknown>) {
    if (busyRef.current) return;
    busyRef.current = true; setBusy(true); setCommandError(null);
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

  return <main className="app-shell" onKeyDown={(event) => { if (event.key === "Escape" && settingsOpen) navigate(false); }}>
    <header className="app-header">
      <div className="wordmark"><span className="brand-mark" aria-hidden="true"><i /><i /><i /></span>OpenKrisp</div>
      {settingsOpen ? <button className="quiet-button" onClick={() => navigate(false)}><AudioIcon kind="back" />Back</button>
        : <button ref={settingsButton} className="quiet-button" onClick={() => navigate(true)}><AudioIcon kind="settings" />Settings</button>}
    </header>

    {fault ? <aside className="notice error" role="alert"><strong>{fault.message}</strong>{fault.suggestedAction ? <span>{fault.suggestedAction}</span> : null}</aside> : null}
    {settingsProblem ? <aside className="notice error" aria-label="Settings persistence">
      {settings.status?.loadWarning ? <><p role="alert">{settings.status.loadWarning.message} Your choices can be used for this session.</p>
        <button disabled={!settings.ready || settings.saving || snapshot?.shuttingDown} onClick={() => void settings.retry(true)}>Replace unreadable settings</button></> : null}
      {settings.error ?? settings.status?.writeError?.message ? <><p role="alert">{settings.error ?? settings.status?.writeError?.message}</p>
        <button disabled={settings.loading || snapshot?.shuttingDown} onClick={() => void settings.retry()}>Retry settings</button></> : null}
    </aside> : null}

    <div hidden={settingsOpen}>
      <section className="microphone-panel" aria-labelledby="microphone-title">
        <div className="microphone-heading">
          <div className={`microphone-symbol ${running ? "is-running" : ""}`}><AudioIcon kind="microphone" /></div>
          <div><h1 id="microphone-title">{title}</h1><p>{running ? selection.destinationKind === "physical_output" ? "Listening through your headphones." : "Keep OpenKrisp open while you’re on a call." : "Clearer speech for your calls."}</p></div>
        </div>
        <div className="device-heading"><label htmlFor="microphone-device">Microphone</label><span className={`status ${running ? "on" : ""}`} role="status"><span className="status-dot" />{status}</span></div>
        <select id="microphone-device" aria-describedby={hasStarted ? "microphone-locked" : undefined} disabled={editingLocked} value={selection.inputId} onChange={(event) => setSelection({ ...selection, inputId: event.target.value })}>
          <option value="">Choose a microphone</option>
          {selection.inputId && !inputs.some((device) => device.id === selection.inputId) ? <option value={selection.inputId}>Saved microphone unavailable</option> : null}
          {inputs.map((device) => <option key={device.id} value={device.id}>{device.name}{device.isDefaultInput ? " (default)" : ""}</option>)}
        </select>
        {hasStarted ? <p id="microphone-locked" className="device-hint">Stop the microphone to change devices.</p> : null}
        <AudioLevel label="Input level" value={route?.metrics.inputPeak ?? 0} active={running} />
        <NoiseReductionSwitch label="Microphone noise reduction" draft={selection} onDraft={setSelection} route={route} locked={controlsLocked || !settings.ready || pendingStart !== null} />
        {!settings.ready && !settingsProblem ? <p role="status" className="hint">{settings.loading ? "Loading saved settings…" : "Waiting for your audio devices…"}</p> : null}
        {!active && settings.ready && selectionError ? <div className="notice"><p>{selectionError}</p><button className="text-button" onClick={() => navigate(true)}>Open audio settings</button></div> : null}
        {selection.destinationKind === "physical_output" ? <p className="notice">You’re in microphone test mode. Use headphones to avoid feedback. Switch to “Call microphone” in Settings to use OpenKrisp in calls.</p> : null}
        <button className={`primary transport ${hasStarted ? "transport-stop" : ""}`}
          disabled={controlsLocked || (hasStarted ? !routeCanStop(route, pendingStart !== null) : !settings.ready || selectionError !== null)}
          onClick={hasStarted ? stop : start}>
          <span className={hasStarted ? "stop-icon" : "start-icon"} aria-hidden="true" />
          {hasStarted ? route?.state === "stopping" ? "Stopping microphone…" : pendingStart !== null && !active ? "Cancel start" : "Stop microphone" : "Start microphone"}
        </button>
        {route?.state === "failed" ? <button className="text-button cleanup-button" disabled={controlsLocked} onClick={stop}>Release audio device</button> : null}
      </section>

      {selection.destinationKind === "virtual_microphone" ? <section className="call-setup" aria-labelledby="call-setup-title">
        <div className="setup-heading"><h2 id="call-setup-title">In your call app</h2><span>One-time setup</span></div>
        <p>Choose this in your app’s audio settings:</p>
        <div className="handoff-device"><span>Microphone</span><strong>{endpoint?.name ?? "Choose a call microphone in Settings"}</strong></div>
        <details className="setup-help"><summary>How to connect</summary><ol>
          <li>Start your microphone in OpenKrisp.</li>
          <li>Open the audio settings in your calling or recording app.</li>
          <li>Select <strong>{endpoint?.name ?? "the OpenKrisp microphone"}</strong> as its microphone. Keep your usual headphones or speakers selected for playback.</li>
        </ol><p>Keep OpenKrisp running during the call. If the device is missing, open Settings for setup help.</p></details>
      </section> : null}
    </div>

    <div hidden={!settingsOpen}>
      <div className="settings-heading"><h1 ref={settingsHeading} tabIndex={-1}>Settings</h1><p>Device setup and advanced audio controls.</p></div>
      <section className="settings-section" aria-labelledby="route-title">
        <div className="panel-heading"><h2 id="route-title">Microphone</h2><button className="quiet-button" disabled={controlsLocked} onClick={() => void runCommand(api.refreshAudioDevices)}>Refresh devices</button></div>
        {hasStarted ? <div className="settings-running"><p>Stop your microphone to change devices or echo cancellation.</p>
          <button disabled={controlsLocked || !routeCanStop(route, pendingStart !== null)} onClick={stop}>Stop microphone</button></div> : null}
        <fieldset disabled={editingLocked}>
          <label>Use microphone for<select value={selection.destinationKind} onChange={(event) => {
            const destinationKind = event.target.value as DestinationKind;
            const outputId = destinationKind === "virtual_microphone" ? snapshot?.routing.endpoints.length === 1 ? snapshot.routing.endpoints[0]!.id : ""
              : outputs.find((device) => device.isDefaultOutput)?.id ?? "";
            setSelection({ ...selection, destinationKind, outputId });
          }}><option value="virtual_microphone" disabled={!snapshot?.routing.capabilities.virtualMicrophone}>Call microphone{snapshot && !snapshot.routing.capabilities.virtualMicrophone ? " (unavailable)" : ""}</option><option value="physical_output">Listen to my microphone</option></select></label>
          <label>{selection.destinationKind === "virtual_microphone" ? "Call microphone" : "Headphones"}<select value={selection.outputId} onChange={(event) => setSelection({ ...selection, outputId: event.target.value })}>
            <option value="">Choose an output</option>
            {selection.destinationKind === "virtual_microphone" ? <>
              {selection.outputId && !endpoint ? <option value={selection.outputId}>Saved call microphone unavailable</option> : null}
              {snapshot?.routing.endpoints.map((endpoint) => <option key={endpoint.id} value={endpoint.id}>{endpoint.name}</option>)}
            </> : <>
              {selection.outputId && !outputs.some((device) => device.id === selection.outputId) ? <option value={selection.outputId}>Saved headphones unavailable</option> : null}
              {outputs.map((device) => <option key={device.id} value={device.id}>{device.name}</option>)}
            </>}
          </select></label>
          <label className="check"><input type="checkbox" checked={selection.echoCancellation} onChange={(event) => setSelection({ ...selection, echoCancellation: event.target.checked })} />Echo cancellation</label>
        </fieldset>
        <p className="hint">Reduces call audio picked up by your microphone when call playback also runs through OpenKrisp.</p>
        <EchoCancellation route={route} enabled={selection.echoCancellation} connected={state.connected} snapshot={snapshot} />
        <DenoiseControls label="Microphone" settings={selection.denoise} onChange={(denoise) => setSelection({ ...selection, denoise })}
          route={route} locked={controlsLocked || !settings.ready || pendingStart !== null}
          startOptions={<fieldset disabled={editingLocked}>
            <label>Microphone processing<select value={selection.mode} onChange={(event) => setSelection({ ...selection, mode: event.target.value as typeof selection.mode })}><option value="speech_denoise">Noise reduction</option><option value="aligned_bypass">Original sound</option></select></label>
            <label>Microphone target latency (ms)<input type="number" min={1} max={1000} step={1} value={Number.isNaN(selection.targetLatencyMs) ? "" : selection.targetLatencyMs} onChange={(event) => setSelection({ ...selection, targetLatencyMs: event.target.value === "" ? NaN : Number(event.target.value) })} /></label>
          </fieldset>} />
      </section>
    </div>

    {snapshot ? <IncomingAudio snapshot={snapshot} connected={state.connected} draft={incomingDraft} onDraft={setIncomingDraft} settingsReady={settings.ready} settingsOpen={settingsOpen} /> : null}

    <div hidden={!settingsOpen}>
      {snapshot ? <details className="settings-section"><summary>Audio device setup</summary><EndpointSetup routing={snapshot.routing} /></details> : null}
      {snapshot ? <DiagnosticPanel snapshot={snapshot} connected={state.connected} /> : null}
      <p className="save-status" role="status" aria-label="Settings persistence">{!settings.ready ? settings.loading ? "Loading saved settings…" : "Waiting for settings and devices…" : settingsProblem ? "Settings are not saved." : settings.saving ? "Saving settings…" : "Settings saved automatically."}</p>
    </div>
    <footer className="app-footer" hidden={settingsOpen}><AudioIcon kind="shield" /><span>Audio is processed on your device.</span></footer>
  </main>;
}
