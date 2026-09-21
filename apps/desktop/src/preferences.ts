import type { DesktopSettings, EngineSnapshot, RoutePreferences } from "./api";
import { defaultDraft, type RouteDraft } from "./drafts";
import { isPipeWireDefaultAlias } from "./routing";

export function preferencesFromDraft({ kind: _kind, ...preferences }: RouteDraft): RoutePreferences {
  return { ...preferences, denoise: { ...preferences.denoise } };
}

export function defaultSettings(): DesktopSettings {
  return { microphone: preferencesFromDraft(defaultDraft("microphone")), incoming: preferencesFromDraft(defaultDraft("incoming")) };
}

/** Resolve only empty preferences, once on startup. Named missing devices stay selected. */
export function initialSettings(settings: DesktopSettings, snapshot: EngineSnapshot): DesktopSettings {
  const endpoints = [...snapshot.routing.endpoints, ...(snapshot.routing.loopbackEndpoints ?? [])];
  const physical = snapshot.inventory.devices.filter((device) => device.kind === "physical" &&
    !isPipeWireDefaultAlias(device.id) && !endpoints.some((endpoint) =>
      endpoint.playbackDeviceId === device.id || endpoint.captureDeviceId === device.id || endpoint.ancestry.recursivePaths.includes(device.id)));
  const input = physical.find((device) => device.supportsInput && device.isDefaultInput)?.id ?? "";
  const output = physical.find((device) => device.supportsOutput && device.isDefaultOutput)?.id ?? "";
  const microphoneEndpoint = snapshot.routing.endpoints.length === 1 ? snapshot.routing.endpoints[0]!.id : "";
  const incomingEndpoints = snapshot.routing.loopbackEndpoints ?? [];
  const speakerEndpoint = incomingEndpoints.length === 1 ? incomingEndpoints[0]!.id : "";
  return {
    microphone: { ...settings.microphone, inputId: settings.microphone.inputId || input,
      outputId: settings.microphone.outputId || (settings.microphone.destinationKind === "virtual_microphone" ? microphoneEndpoint : output) },
    incoming: { ...settings.incoming, inputId: settings.incoming.inputId || speakerEndpoint,
      outputId: settings.incoming.outputId || output },
  };
}

export function sameSettings(a: unknown, b: unknown): boolean { const encode = (value: unknown) => JSON.stringify(value, (_key, entry: unknown) =>
  typeof entry === "number" && !Number.isFinite(entry) ? String(entry) : entry);
  return encode(a) === encode(b); }
