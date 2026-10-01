import type { DesktopSettings, EngineSnapshot, RoutePreferences } from "./api";
import { defaultDraft, type RouteDraft } from "./drafts";
import { isPipeWireDefaultAlias } from "./routing";

export function preferencesFromDraft({ kind: _kind, ...preferences }: RouteDraft): RoutePreferences {
  return { ...preferences, denoise: { ...preferences.denoise } };
}

export function defaultSettings(): DesktopSettings {
  return { microphone: preferencesFromDraft(defaultDraft("microphone")), incoming: preferencesFromDraft(defaultDraft("incoming")) };
}

/** The desktop owns tuning, noise reduction and virtual routing; users choose physical devices. */
export function automaticDraft(draft: RouteDraft, snapshot: EngineSnapshot): RouteDraft {
  const microphone = draft.kind === "microphone";
  const endpoints = microphone ? snapshot.routing.endpoints : snapshot.routing.loopbackEndpoints ?? [];
  const endpointId = endpoints.length === 1 ? endpoints[0]!.id : "";
  return { ...defaultDraft(draft.kind),
    inputId: microphone ? draft.inputId : endpointId,
    outputId: microphone ? endpointId : draft.outputId,
    mode: "speech_denoise",
  };
}

/** Preserve physical device choices, including missing devices; retire manual tuning/routing. */
export function initialSettings(settings: DesktopSettings, snapshot: EngineSnapshot): DesktopSettings {
  const endpoints = [...snapshot.routing.endpoints, ...(snapshot.routing.loopbackEndpoints ?? [])];
  const physical = snapshot.inventory.devices.filter((device) => device.kind === "physical" &&
    !isPipeWireDefaultAlias(device.id) && !endpoints.some((endpoint) =>
      endpoint.playbackDeviceId === device.id || endpoint.captureDeviceId === device.id || endpoint.ancestry.recursivePaths.includes(device.id)));
  const input = physical.find((device) => device.supportsInput && device.isDefaultInput)?.id ?? "";
  const output = physical.find((device) => device.supportsOutput && device.isDefaultOutput)?.id ?? "";
  return {
    microphone: preferencesFromDraft(automaticDraft({ ...settings.microphone, kind: "microphone", inputId: settings.microphone.inputId || input }, snapshot)),
    incoming: preferencesFromDraft(automaticDraft({ ...settings.incoming, kind: "incoming", outputId: settings.incoming.outputId || output }, snapshot)),
  };
}

export function sameSettings(a: unknown, b: unknown): boolean { const encode = (value: unknown) => JSON.stringify(value, (_key, entry: unknown) =>
  typeof entry === "number" && !Number.isFinite(entry) ? String(entry) : entry);
  return encode(a) === encode(b); }
