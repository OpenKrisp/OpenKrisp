import type { RouteSnapshot } from "./api";
import type { RouteDraft } from "./drafts";
import { routeIsActive } from "./routing";

export function AudioIcon({ kind, className = "" }: { kind: "microphone" | "headphones" | "help" | "back" | "shield"; className?: string }) {
  return <svg className={className} width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {kind === "microphone" ? <><rect x="8" y="3" width="8" height="12" rx="4" /><path d="M5 11v1a7 7 0 0 0 14 0v-1M12 19v3M9 22h6" /></> : null}
    {kind === "headphones" ? <><path d="M4 14v-3a8 8 0 0 1 16 0v3" /><rect x="3" y="12" width="4" height="8" rx="2" /><rect x="17" y="12" width="4" height="8" rx="2" /></> : null}
    {kind === "help" ? <><circle cx="12" cy="12" r="9" /><path d="M9.5 9a2.5 2.5 0 1 1 4.2 1.8c-1 .7-1.7 1.1-1.7 2.7M12 17h.01" /></> : null}
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
  // Engine peaks are linear amplitudes. Map -60…0 dBFS onto the bar so speech
  // below full scale is visible; a 0.01 peak (-40 dBFS) fills one third, not 1%.
  // The engine already applies peak release and publishes metrics every 100 ms.
  const floorDb = -60;
  const peak = active && Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
  const decibels = peak > 0 ? Math.max(floorDb, 20 * Math.log10(peak)) : floorDb;
  const level = (decibels - floorDb) / -floorDb;
  const description = !active ? "Off" : level === 0 ? "Quiet" : `${Math.round(decibels)} dBFS`;
  return <div className="audio-level">
    <span>{label}</span>
    <meter aria-label={label} aria-valuetext={description} min={0} max={1} value={level} />
  </div>;
}

export function noiseReductionEnabled(draft: RouteDraft, route: RouteSnapshot | undefined): boolean {
  return routeIsActive(route) && route ? !route.bypassed && route.config.denoise.enabled && route.config.processingMode === "speech_denoise"
    : draft.mode === "speech_denoise" && draft.denoise.enabled;
}
