import type { EngineSnapshot, ProcessingMode, StartRouteRequest } from "./api";
import { isPipeWireDefaultAlias } from "./routing";

export const incomingRouteId = "desktop-incoming";

export function incomingSelectionError(snapshot: EngineSnapshot, sourceId: string, outputId: string): string | null {
  if (!snapshot.routing.capabilities.systemLoopback) return "Incoming loopback audio is unavailable in this build.";
  const source = snapshot.routing.loopbackEndpoints?.find((endpoint) => endpoint.id === sourceId);
  if (!source) return "Choose an incoming audio endpoint.";
  if (source.availability !== "ready" && !(source.availability === "missing" && source.canCreate)) return `Set up ${source.name}, then refresh devices.`;
  const output = snapshot.inventory.devices.find((device) => device.id === outputId && device.supportsOutput);
  if (!output || output.kind !== "physical" || isPipeWireDefaultAlias(outputId)) return "Choose available physical headphones or speakers.";
  if (source.ancestry.recursivePaths.includes(outputId)) return "The output cannot be part of the incoming capture path.";
  for (const route of snapshot.routes) {
    if (!("physicalInput" in route.config.source) || route.state === "stopped") continue;
    const destination = route.config.destination;
    if ("virtualMicrophone" in destination && destination.virtualMicrophone === sourceId) return "Stop the virtual microphone before using this same endpoint for incoming audio.";
    if ("physicalOutput" in destination && destination.physicalOutput === outputId) return "Choose separate headphones or stop the microphone monitor using this output.";
  }
  return null;
}

export function incomingStartRequest(sourceId: string, outputId: string, mode: ProcessingMode = "aligned_bypass"): StartRouteRequest {
  return { routeId: incomingRouteId, sourceKind: "system_loopback", sourceDeviceId: sourceId,
    destinationKind: "physical_output", destinationDeviceId: outputId, mode, targetLatencyMs: 40,
    denoise: { enabled: true, attenuationLimitDb: null, postFilterEnabled: false }, echoCancellation: false };
}
