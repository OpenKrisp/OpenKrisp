import { useEffect, useRef, useState } from "react";
import { api, type RouteSnapshot } from "./api";
import type { RouteDraft } from "./drafts";
import { routeIsActive } from "./routing";
import { normalizeApiError } from "./state";

export function AudioIcon({ kind, className = "" }: { kind: "microphone" | "headphones" | "settings" | "back" | "shield"; className?: string }) {
  return <svg className={className} width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {kind === "microphone" ? <><rect x="8" y="3" width="8" height="12" rx="4" /><path d="M5 11v1a7 7 0 0 0 14 0v-1M12 19v3M9 22h6" /></> : null}
    {kind === "headphones" ? <><path d="M4 14v-3a8 8 0 0 1 16 0v3" /><rect x="3" y="12" width="4" height="8" rx="2" /><rect x="17" y="12" width="4" height="8" rx="2" /></> : null}
    {kind === "settings" ? <><path d="M4 7h5m5 0h6M4 17h10m5 0h1" /><circle cx="11.5" cy="7" r="2.5" /><circle cx="16.5" cy="17" r="2.5" /></> : null}
    {kind === "back" ? <path d="m10 5-7 7 7 7M3 12h18" /> : null}
    {kind === "shield" ? <><path d="m12 3 8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3Z" /><path d="m8 12 3 3 5-6" /></> : null}
  </svg>;
}

export function audioStatus(route: RouteSnapshot | undefined, connected: boolean, pending = false): string {
  if (!connected) return "Reconnecting…";
  if (pending && !routeIsActive(route)) return "Starting…";
  switch (route?.state) {
    case "starting": return "Starting…";
    case "running": return "On";
    case "recovering": return "Reconnecting audio…";
    case "reconfiguring": return "Changing audio device…";
    case "stopping": return "Stopping…";
    case "failed": return "Needs attention";
    default: return "Off";
  }
}

export function AudioLevel({ value, active, label }: { value: number; active: boolean; label: string }) {
  const level = active && Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
  return <div className="audio-level">
    <span>{label}</span>
    <meter aria-label={label} min={0} max={1} value={level} />
  </div>;
}

export function NoiseReductionSwitch({ label, draft, onDraft, route, locked }: {
  label: string; draft: RouteDraft; onDraft: (draft: RouteDraft) => void;
  route: RouteSnapshot | undefined; locked: boolean;
}) {
  const active = routeIsActive(route);
  const on = active && route ? !route.bypassed && route.config.denoise.enabled && route.config.processingMode === "speech_denoise"
    : draft.mode === "speech_denoise" && draft.denoise.enabled;
  const [pending, setPending] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const busy = useRef(false);
  const [requestBusy, setRequestBusy] = useState(false);

  useEffect(() => {
    if (pending !== null && (on === pending || !active || route?.fault)) setPending(null);
  }, [on, active, route?.fault, pending]);
  useEffect(() => {
    if (pending === null) return;
    const timer = setTimeout(() => { setPending(null); setError("The change was not confirmed. Try again."); }, 5000);
    return () => clearTimeout(timer);
  }, [pending]);

  async function toggle() {
    if (locked || busy.current || pending !== null || (active && route?.state !== "running")) return;
    setError(null);
    if (!active) {
      onDraft({ ...draft, mode: on ? "aligned_bypass" : "speech_denoise", denoise: { ...draft.denoise, enabled: true } });
      return;
    }
    if (!route) return;
    busy.current = true; setRequestBusy(true); setPending(!on);
    try {
      if (!on && !route.config.denoise.enabled) await api.updateRouteSettings(route.config.id, { ...route.config.denoise, enabled: true });
      await api.setRouteBypass(route.config.id, on);
    } catch (failure) { setError(normalizeApiError(failure).message); setPending(null); }
    finally { busy.current = false; setRequestBusy(false); }
  }

  return <div className="noise-control">
    <div className="switch-row">
      <div><span className="control-title">Noise reduction</span><p>{draft.kind === "microphone" ? "Keep background sounds out of your calls." : "Clearer voices. Leave off for music."}</p></div>
      <button className="switch" role="switch" aria-label={label} aria-checked={on}
        disabled={locked || requestBusy || pending !== null || (active && route?.state !== "running")}
        onClick={() => void toggle()}><span className="switch-thumb" /></button>
    </div>
    {pending !== null || requestBusy ? <p className="hint" role="status">Changing noise reduction…</p> : null}
    {route?.metrics.degraded && active ? <p className="notice" role="status">Noise reduction is temporarily paused to keep your audio playing.</p> : null}
    {error ? <p className="notice error" role="alert">{error}</p> : null}
  </div>;
}
