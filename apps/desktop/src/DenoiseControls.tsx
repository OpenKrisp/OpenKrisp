import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, type DenoiseSettings, type RouteSnapshot } from "./api";
import { denoiseEqual, denoiseError } from "./drafts";
import { normalizeApiError } from "./state";
import { routeIsActive } from "./routing";

interface Props {
  label: string;
  settings: DenoiseSettings;
  onChange: (settings: DenoiseSettings) => void;
  route: RouteSnapshot | undefined;
  locked: boolean;
  startOptions?: ReactNode;
}

export function DenoiseControls({ label, settings, onChange, route, locked, startOptions }: Props) {
  const [pending, setPending] = useState<DenoiseSettings | null>(null);
  const [requestBusy, setRequestBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const busy = useRef(false);
  const currentRoute = useRef(route);
  currentRoute.current = route;
  const error = denoiseError(settings);
  const changed = !denoiseEqual(route?.config.denoise, settings);
  const active = routeIsActive(route);

  useEffect(() => {
    if (!pending) return;
    if (route?.state !== "running" || route.fault) {
      setPending(null);
      setMessage("Settings were not confirmed. Check the route status before applying again.");
    } else if (denoiseEqual(route.config.denoise, pending)) {
      setPending(null);
      setMessage("The engine accepted these settings.");
    }
  }, [route, pending]);
  useEffect(() => {
    if (!pending) return;
    const timer = setTimeout(() => {
      setPending(null);
      setMessage("Settings have not been confirmed yet. Check the connection and try again.");
    }, 5000);
    return () => clearTimeout(timer);
  }, [pending]);

  async function apply() {
    if (busy.current || locked || pending || error || route?.state !== "running") return;
    const id = route.config.id;
    const submitted = { ...settings };
    busy.current = true;
    setRequestBusy(true);
    setMessage(null);
    setPending(submitted);
    try { await api.updateRouteSettings(id, submitted); }
    catch (failure) {
      if (currentRoute.current?.config.id === id) {
        setPending(null);
        setMessage(normalizeApiError(failure).message);
      }
    } finally { busy.current = false; setRequestBusy(false); }
  }

  return <details className="settings">
    <summary>{label} settings</summary>
    {startOptions}
    <fieldset disabled={locked || requestBusy || pending !== null || (active && route?.state !== "running")}>
      <label className="check"><input type="checkbox" checked={settings.enabled}
        onChange={(event) => onChange({ ...settings, enabled: event.target.checked })} />Use noise reduction</label>
      <label className="check"><input type="checkbox" checked={settings.attenuationLimitDb !== null}
        onChange={(event) => onChange({ ...settings, attenuationLimitDb: event.target.checked ? 24 : null })} />Limit maximum reduction</label>
      {settings.attenuationLimitDb !== null ? <label>Maximum reduction (dB)
        <input type="number" min={0} step="any" value={Number.isNaN(settings.attenuationLimitDb) ? "" : settings.attenuationLimitDb}
          onChange={(event) => onChange({ ...settings, attenuationLimitDb: event.target.value === "" ? NaN : Number(event.target.value) })} />
      </label> : <p className="hint">Maximum reduction is unlimited.</p>}
      <label className="check"><input type="checkbox" checked={settings.postFilterEnabled}
        onChange={(event) => onChange({ ...settings, postFilterEnabled: event.target.checked })} />Additional post-filter</label>
    </fieldset>
    <p className="hint">Lower limits preserve more background sound. The post-filter adds suppression and may affect voice quality. Bypass temporarily skips noise reduction.</p>
    {error ? <p role="alert">{error}</p> : null}
    {active ? <button disabled={locked || requestBusy || pending !== null || !changed || error !== null || route?.state !== "running"}
      onClick={() => void apply()}>{pending || requestBusy ? "Applying settings…" : "Apply noise reduction settings"}</button>
      : <p className="hint">These settings will be used when you start this route.</p>}
    {message ? <p role="status">{message}</p> : null}
  </details>;
}
