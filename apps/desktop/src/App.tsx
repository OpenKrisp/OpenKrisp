import { useI18n, Message, I18nProvider, LanguageSelector, ErrorDetails } from "./i18n";
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
import { useUpdates } from "./useUpdates";
import { UpdateNotice } from "./UpdateNotice";
import { draftSettingsError, requestFromDraft } from "./drafts";
import { useRouteDraft } from "./useRouteDraft";
import { isPipeWireDefaultAlias, routeCanStop, routeIsActive, routeSelectionError, routeToStop } from "./routing";
import { initialDesktopState, normalizeApiError, reduceDesktopState } from "./state";

export function App() {
  return <I18nProvider><DesktopApp /></I18nProvider>;
}

function DesktopApp() {
  const { t, language, error: displayError } = useI18n();
  const updates = useUpdates();
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
  const selectionError = snapshot ? (routeSelectionError(snapshot, selection, t) ?? draftSettingsError(selection, t)) : null;
  const rawFault = commandError ?? state.fault ?? route?.fault;
  const fault = rawFault ? displayError(rawFault) : null;
  const settingsError = settings.error ?? settings.status?.writeError;
  const controlsLocked = busy || updates.installing || !state.connected || snapshot?.shuttingDown === true;
  const editingLocked = controlsLocked || !settings.ready || active || pendingStart !== null;
  const hasStarted = active || pendingStart !== null;
  const starting = route?.state === "starting" || (pendingStart !== null && !active);
  const actionLabel = !hasStarted ? t("common.turnOn") : route?.state === "stopping" ? t("common.turningOff")
    : starting ? t("common.cancelStart") : t("common.turnOff");
  const running = state.connected && route?.state === "running";
  const status = audioStatus(route, state.connected, pendingStart !== null, t);
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
    const routeId = routeToStop("microphone", route, pendingStart !== null);
    if (!routeId) return;
    void runCommand(async () => { await api.stopRoute(routeId); setPendingStart(null); });
  }

  return <main className={`app-shell ${helpOpen ? "is-help" : ""}`} onKeyDown={(event) => { if (event.key === "Escape" && helpOpen) navigate(false); }}>
    <header className="app-header">
      <div className="wordmark"><span className="brand-mark" aria-hidden="true"><i /><i /><i /></span>OpenKrisp</div>
      <div className="header-actions"><LanguageSelector />
        {helpOpen ? <button className="quiet-button" aria-label={t("common.back")} title={t("common.back")} onClick={() => navigate(false)}><AudioIcon kind="back" /><span className="header-action-label">{t("common.back")}</span></button>
          : <button ref={helpButton} className="quiet-button" aria-label={t("common.help")} title={t("common.help")} onClick={() => navigate(true)}><AudioIcon kind="help" /><span className="header-action-label">{t("common.help")}</span></button>}</div>
    </header>

    <UpdateNotice updates={updates} helpOpen={helpOpen} audioBusy={busy || !state.connected || !settings.ready || settings.saving ||
      pendingStart !== null || snapshot?.shuttingDown === true || (snapshot?.routes.some((route) => route.state !== "stopped") ?? true)} />

    {fault ? <aside className="notice error" role="alert"><strong>{fault.message}</strong>{fault.suggestedAction ? <span>{fault.suggestedAction}</span> : null}{rawFault ? <ErrorDetails error={rawFault} /> : null}</aside> : null}
    {settingsProblem ? <aside className="notice error" aria-label={t("settings.persistence")}>
      {settings.status?.loadWarning ? <><p role="alert">{t("settings.session", { message: language === "en" ? settings.status.loadWarning.message : t("settings.unreadable") })}</p><ErrorDetails error={settings.status.loadWarning} />
        <button disabled={!settings.ready || settings.saving || snapshot?.shuttingDown} onClick={() => void settings.retry(true)}>{t("settings.replace")}</button></> : null}
      {settingsError ? <><p role="alert">{language === "en" ? settingsError.message : t(settings.ready ? "settings.writeFailed" : "settings.readFailed")}</p><ErrorDetails error={settingsError} />
        <button disabled={settings.loading || snapshot?.shuttingDown} onClick={() => void settings.retry()}>{t("settings.retry")}</button></> : null}
    </aside> : null}

    <h1 className="sr-only" hidden={helpOpen}>{t("app.title")}</h1>
    <div className="help-heading" hidden={!helpOpen}><h1 ref={helpHeading} tabIndex={-1}>{t("common.help")}</h1></div>
    <div className={helpOpen ? "help-audio-status" : "audio-workspace"}>
      <section className="audio-panel microphone-panel" aria-labelledby="microphone-title" hidden={helpOpen}>
        <div className="audio-panel-heading">
          <h2 id="microphone-title">{t("microphone.title")}</h2>
          <span className={`status ${running ? "on" : ""}`} role="status"><span className={starting && state.connected ? "status-spinner" : "status-dot"} aria-hidden="true" />{status}</span>
        </div>
        <p className="audio-panel-description">{t("microphone.description")}</p>
        <MicrophoneFlow draft={active ? selection : startSelection} route={route} running={running} waiting={!state.connected || !settings.ready || (hasStarted && !running)} />
        <div className="device-heading"><label htmlFor="microphone-device">{t("microphone.device")}</label></div>
        <select id="microphone-device" aria-describedby={hasStarted ? "microphone-locked" : undefined} disabled={editingLocked} value={selection.inputId} onChange={(event) => setSelection({ ...selection, inputId: event.target.value })}>
          <option value="">{t("microphone.choose")}</option>
          {selection.inputId && !inputs.some((device) => device.id === selection.inputId) ? <option value={selection.inputId}>{t("microphone.unavailable")}</option> : null}
          {inputs.map((device) => <option key={device.id} value={device.id}>{device.name}{device.isDefaultInput ? t("microphone.default") : ""}</option>)}
        </select>
        {hasStarted ? <p id="microphone-locked" className="device-hint">{starting && state.connected ? t("common.preparing") : t("common.changeDevices")}</p> : null}
        <AudioLevel label={t("microphone.level")} value={route?.metrics.inputPeak ?? 0} active={running} />
        {running && route?.metrics.degraded ? <p className="notice" role="status">{t("common.degraded")}</p> : null}
        {!settings.ready && !settingsProblem ? <p role="status" className="hint">{settings.loading ? t("settings.loading") : t("common.waitingDevices")}</p> : null}
        {!active && settings.ready && selectionError ? <div className="notice"><p>{selectionError}</p><button className="text-button" onClick={() => navigate(true)}>{t("common.setupHelp")}</button></div> : null}
        {selection.destinationKind === "physical_output" ? <p className="notice">{t("microphone.testMode")}</p> : null}
        <button className={`primary transport ${hasStarted ? "transport-stop" : ""}`}
          disabled={controlsLocked || (hasStarted ? !routeCanStop(route, pendingStart !== null) : !settings.ready || selectionError !== null)}
          onClick={hasStarted ? stop : start}>
          <span className={hasStarted ? "stop-icon" : "start-icon"} aria-hidden="true" />
          {actionLabel}
        </button>
        {route?.state === "failed" ? <button className="text-button cleanup-button" disabled={controlsLocked} onClick={stop}>{t("microphone.release")}</button> : null}
      {selection.destinationKind === "virtual_microphone" ? <section className="call-setup" aria-labelledby="call-setup-title">
        <AudioIcon kind="microphone" className="handoff-icon" />
        <div className="call-setup-content">
          <h3 id="call-setup-title">{t("microphone.callDevice")}</h3>
          <p className="call-device-name">{endpoint?.name ?? "OpenKrisp Microphone"}</p>
          <details className="setup-help"><summary>{t("common.connect")}</summary><ol>
            <li>{t("common.startStep")}</li>
            <li>{t("microphone.openSettings")}</li>
            <li><Message id="microphone.select" values={{ device: <strong>{endpoint?.name ?? t("microphone.fallbackDevice")}</strong> }} /></li>
          </ol><p>{t("microphone.keepRunning")}</p></details>
        </div>
      </section> : null}
      </section>

      {hasStarted ? <section className="settings-running" aria-label={t("microphone.status")} hidden={!helpOpen}><span className={`status ${running ? "on" : ""}`} role="status"><span className={starting && state.connected ? "status-spinner" : "status-dot"} aria-hidden="true" />{t("microphone.statusLabel", { status })}</span>
        <button disabled={controlsLocked || !routeCanStop(route, pendingStart !== null)} onClick={stop}>{actionLabel}</button></section> : null}

      {snapshot ? <IncomingAudio snapshot={snapshot} connected={state.connected && !updates.installing} draft={incomingDraft} onDraft={setIncomingDraft} settingsReady={settings.ready}
        helpOpen={helpOpen} onOpenHelp={() => navigate(true)} /> : <section className="audio-panel" hidden={helpOpen} aria-labelledby="incoming-loading-title">
          <div className="audio-panel-heading"><h2 id="incoming-loading-title">{t("speakers.title")}</h2></div>
          <p className="audio-panel-description">{t("speakers.description")}</p>
          <p role="status">{t("common.waitingDevices")}</p>
        </section>}
    </div>

    <div hidden={!helpOpen}>
      {snapshot ? <EndpointSetup routing={snapshot.routing} /> : <p className="connection-guide" role="status">{t("common.waitingDevices")}</p>}
      <section className="help-troubleshooting" aria-labelledby="troubleshooting-title">
        <h2 id="troubleshooting-title">{t("help.troubleshooting")}</h2>
        <section className="help-action" aria-labelledby="refresh-title">
          <div className="help-action-description"><h3 id="refresh-title">{t("help.missingDevice")}</h3><p>{t("help.reconnect")}</p></div>
          <button className="secondary" disabled={controlsLocked} onClick={() => void runCommand(api.refreshAudioDevices)}>{t("common.refresh")}</button>
        </section>
        {snapshot ? <SupportDetails snapshot={snapshot} connected={state.connected} /> : null}
      </section>
    </div>
    <footer className="app-footer" hidden={helpOpen}><AudioIcon kind="shield" /><span>{t("app.privacy")}</span></footer>
  </main>;
}
