import type { DenoiseSettings, ProcessingMode, RouteSnapshot, StartRouteRequest } from "./api";
import type { RouteSelection } from "./routing";

export type RouteKind = "microphone" | "incoming";
export interface RouteDraft extends RouteSelection {
  kind: RouteKind;
  mode: ProcessingMode;
  targetLatencyMs: number;
  denoise: DenoiseSettings;
  echoCancellation: boolean;
}

export function defaultDraft(kind: RouteKind): RouteDraft {
  return {
    kind, inputId: "", outputId: "",
    destinationKind: kind === "microphone" ? "virtual_microphone" : "physical_output",
    mode: kind === "microphone" ? "speech_denoise" : "aligned_bypass",
    targetLatencyMs: 40,
    denoise: { enabled: true, attenuationLimitDb: null, postFilterEnabled: false },
    echoCancellation: kind === "microphone",
  };
}

export function draftFromRoute(route: RouteSnapshot): RouteDraft | null {
  const source = route.config.source;
  if ("applicationStream" in source) return null;
  const destination = route.config.destination;
  return {
    kind: "physicalInput" in source ? "microphone" : "incoming",
    inputId: "physicalInput" in source ? source.physicalInput : source.systemLoopback,
    outputId: "physicalOutput" in destination ? destination.physicalOutput : destination.virtualMicrophone,
    destinationKind: "physicalOutput" in destination ? "physical_output" : "virtual_microphone",
    mode: route.config.processingMode,
    targetLatencyMs: route.config.targetLatencyMs,
    denoise: { ...route.config.denoise },
    echoCancellation: "physicalInput" in source && (route.config.echoCancellation ?? true),
  };
}

export function denoiseError(settings: DenoiseSettings): string | null {
  const limit = settings.attenuationLimitDb;
  // Reject values which cannot survive the Rust f32 DTO conversion too.
  return limit !== null && (!Number.isFinite(Math.fround(limit)) || limit < 0)
    ? "Maximum reduction must be a finite, non-negative number of decibels." : null;
}

export function denoiseEqual(a: DenoiseSettings | undefined, b: DenoiseSettings): boolean {
  const value = (limit: number | null) => limit === null ? null : Math.fround(limit);
  return a !== undefined && a.enabled === b.enabled && a.postFilterEnabled === b.postFilterEnabled &&
    value(a.attenuationLimitDb) === value(b.attenuationLimitDb);
}

export function draftSettingsError(draft: RouteDraft): string | null {
  if (!Number.isInteger(draft.targetLatencyMs) || draft.targetLatencyMs < 1 || draft.targetLatencyMs > 1000) {
    return "Target latency must be a whole number from 1 to 1000 milliseconds.";
  }
  return denoiseError(draft.denoise);
}

export function requestFromDraft(draft: RouteDraft): StartRouteRequest {
  return {
    routeId: draft.kind === "microphone" ? "desktop-microphone" : "desktop-incoming",
    sourceKind: draft.kind === "microphone" ? "physical_input" : "system_loopback",
    sourceDeviceId: draft.inputId,
    destinationKind: draft.destinationKind,
    destinationDeviceId: draft.outputId,
    mode: draft.mode, targetLatencyMs: draft.targetLatencyMs,
    denoise: { ...draft.denoise },
    echoCancellation: draft.kind === "microphone" && draft.echoCancellation,
  };
}
