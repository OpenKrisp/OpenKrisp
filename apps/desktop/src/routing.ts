import { englishTranslator, type Translate } from "./translations";
import type { EngineSnapshot, RouteSnapshot } from "./api";
import { desktopRouteIds, type RouteKind } from "./drafts";

export type DestinationKind = "physical_output" | "virtual_microphone";

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

export function routeToStop(kind: RouteKind, route: RouteSnapshot | undefined, startPending: boolean): string | undefined {
  // The last snapshot may still describe a different stopped route while a new start is queued.
  return startPending ? desktopRouteIds[kind] : route?.config.id;
}

export function routeSelectionError(snapshot: EngineSnapshot, selection: RouteSelection, t: Translate = englishTranslator): string | null {
  for (const route of snapshot.routes) {
    if ("physicalInput" in route.config.source || route.state === "stopped") continue;
    if (selection.destinationKind === "virtual_microphone" && "systemLoopback" in route.config.source && route.config.source.systemLoopback === selection.outputId) {
      return t("validation.stopIncoming");
    }
    if (selection.destinationKind === "physical_output" && "physicalOutput" in route.config.destination && route.config.destination.physicalOutput === selection.outputId) {
      return t("validation.separateMonitor");
    }
  }
  if (isPipeWireDefaultAlias(selection.inputId) || isPipeWireDefaultAlias(selection.outputId)) {
    return t("validation.specificDevice");
  }
  if (selection.destinationKind === "virtual_microphone" && !snapshot.routing.capabilities.virtualMicrophone) {
    return t("validation.virtualUnavailable");
  }
  const input = snapshot.inventory.devices.find((device) => device.id === selection.inputId && device.supportsInput);
  if (!input) return t("validation.chooseMicrophone");
  if (selection.destinationKind === "virtual_microphone") {
    if (input.kind !== "physical") return t("validation.physicalDirect");
    const endpoint = snapshot.routing.endpoints.find((candidate) => candidate.id === selection.outputId);
    if (!endpoint) return t("validation.microphoneMissing");
    if (endpoint.availability !== "ready" && !(endpoint.availability === "missing" && endpoint.canCreate)) return t("validation.setupMicrophone", { device: endpoint.name });
    if (endpoint.playbackDeviceId === input.id || endpoint.ancestry.recursivePaths.includes(input.id)) return t("validation.recapture");
  } else {
    if (input.kind === "virtual") return t("validation.physicalMicrophone");
    const output = snapshot.inventory.devices.find((device) => device.id === selection.outputId && device.supportsOutput && device.kind !== "virtual");
    if (!output) return t("validation.headphones");
  }
  return null;
}
