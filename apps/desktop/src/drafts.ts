import { englishTranslator, type Translate } from "./translations";
import type { DenoiseSettings, RoutePreferences, RouteSnapshot, StartRouteRequest } from "./api";

export type RouteKind = "microphone" | "incoming";
export const desktopRouteIds = {
  microphone: "desktop-microphone",
  incoming: "desktop-incoming",
} as const satisfies Record<RouteKind, string>;

export interface RouteDraft extends RoutePreferences {
  kind: RouteKind;
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

export function denoiseError(settings: DenoiseSettings, t: Translate = englishTranslator): string | null {
  const limit = settings.attenuationLimitDb;
  // Reject values which cannot survive the Rust f32 DTO conversion too.
  return limit !== null && (!Number.isFinite(Math.fround(limit)) || limit < 0)
    ? t("validation.reduction") : null;
}

export function denoiseEqual(a: DenoiseSettings, b: DenoiseSettings): boolean {
  const value = (limit: number | null) => limit === null ? null : Math.fround(limit);
  const left = value(a.attenuationLimitDb);
  const right = value(b.attenuationLimitDb);
  return a.enabled === b.enabled && a.postFilterEnabled === b.postFilterEnabled &&
    (left === right || (Number.isNaN(left) && Number.isNaN(right)));
}

export function draftSettingsError(draft: RouteDraft, t: Translate = englishTranslator): string | null {
  if (!Number.isInteger(draft.targetLatencyMs) || draft.targetLatencyMs < 1 || draft.targetLatencyMs > 1000) {
    return t("validation.latency");
  }
  return denoiseError(draft.denoise, t);
}

export function requestFromDraft(draft: RouteDraft): StartRouteRequest {
  return {
    routeId: desktopRouteIds[draft.kind],
    sourceKind: draft.kind === "microphone" ? "physical_input" : "system_loopback",
    sourceDeviceId: draft.inputId,
    destinationKind: draft.destinationKind,
    destinationDeviceId: draft.outputId,
    mode: draft.mode, targetLatencyMs: draft.targetLatencyMs,
    denoise: { ...draft.denoise },
    echoCancellation: draft.kind === "microphone" && draft.echoCancellation,
  };
}
