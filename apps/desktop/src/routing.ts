import type { EngineSnapshot, RouteSnapshot, StartRouteRequest } from "./api";

export type DestinationKind = "physical_output" | "virtual_microphone";
const desktopRouteId = "desktop-microphone";

export interface RouteSelection {
  inputId: string;
  outputId: string;
  destinationKind: DestinationKind;
}

export function routeIsActive(route: RouteSnapshot | undefined): boolean {
  return route !== undefined && route.state !== "stopped" && route.state !== "failed";
}

export function routeCanStop(route: RouteSnapshot | undefined, startPending: boolean): boolean {
  // Failed routes can still own virtual endpoints and must permit explicit cleanup.
  return route?.state !== "stopping" && (startPending || (route !== undefined && route.state !== "stopped"));
}

export function isPipeWireDefaultAlias(id: string): boolean {
  return id === "pipewire:input_default" || id === "pipewire:output_default";
}

export function routeToStop(route: RouteSnapshot | undefined, startPending: boolean): string | undefined {
  // The last snapshot may still describe a different stopped route while a new start is queued.
  return startPending ? desktopRouteId : route?.config.id;
}

export function selectionFromRoute(route: RouteSnapshot): RouteSelection {
  const destination = route.config.destination;
  return {
    inputId: "physicalInput" in route.config.source ? route.config.source.physicalInput : "",
    outputId: "virtualMicrophone" in destination ? destination.virtualMicrophone : destination.physicalOutput,
    destinationKind: "virtualMicrophone" in destination ? "virtual_microphone" : "physical_output",
  };
}

export function routeSelectionError(snapshot: EngineSnapshot, selection: RouteSelection): string | null {
  for (const route of snapshot.routes) {
    if ("physicalInput" in route.config.source || route.state === "stopped") continue;
    if (selection.destinationKind === "virtual_microphone" && "systemLoopback" in route.config.source && route.config.source.systemLoopback === selection.outputId) {
      return "Stop incoming audio before using this same endpoint for the virtual microphone.";
    }
    if (selection.destinationKind === "physical_output" && "physicalOutput" in route.config.destination && route.config.destination.physicalOutput === selection.outputId) {
      return "Choose a separate monitor output or stop incoming audio using this output.";
    }
  }
  if (isPipeWireDefaultAlias(selection.inputId) || isPipeWireDefaultAlias(selection.outputId)) {
    return "Choose a specific microphone and headphone output instead of a PipeWire default alias.";
  }
  if (selection.destinationKind === "virtual_microphone" && !snapshot.routing.capabilities.virtualMicrophone) {
    return "Virtual microphone routing is unavailable in this build.";
  }
  const input = snapshot.inventory.devices.find((device) => device.id === selection.inputId && device.supportsInput);
  if (!input) return "Choose an available microphone.";
  if (selection.destinationKind === "virtual_microphone") {
    if (input.kind !== "physical") return "Choose a physical microphone directly. Virtual, aggregate and unclassified inputs cannot be used here.";
    const endpoint = snapshot.routing.endpoints.find((candidate) => candidate.id === selection.outputId);
    if (!endpoint) return "Choose a virtual microphone from the setup panel.";
    if (endpoint.availability !== "ready" && !(endpoint.availability === "missing" && endpoint.canCreate)) return `Set up ${endpoint.name}, then refresh devices before starting.`;
    if (endpoint.playbackDeviceId === input.id || endpoint.ancestry.recursivePaths.includes(input.id)) return "The microphone cannot capture the processed output.";
  } else {
    if (input.kind === "virtual") return "Choose a physical microphone.";
    const output = snapshot.inventory.devices.find((device) => device.id === selection.outputId && device.supportsOutput && device.kind !== "virtual");
    if (!output) return "Choose an available headphone output.";
  }
  return null;
}

export function startRequest(selection: RouteSelection): StartRouteRequest {
  return {
    routeId: desktopRouteId,
    sourceDeviceId: selection.inputId,
    destinationDeviceId: selection.outputId,
    destinationKind: selection.destinationKind,
    mode: "speech_denoise",
    targetLatencyMs: 40,
    denoise: { enabled: true, attenuationLimitDb: null, postFilterEnabled: false },
    echoCancellation: true,
  };
}
