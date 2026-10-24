import { englishTranslator, type Translate } from "./translations";
import type { EngineSnapshot } from "./api";
import { isPipeWireDefaultAlias } from "./routing";

export function incomingSelectionError(snapshot: EngineSnapshot, sourceId: string, outputId: string, t: Translate = englishTranslator): string | null {
  if (!snapshot.routing.capabilities.systemLoopback) return t("validation.callUnavailable");
  const source = snapshot.routing.loopbackEndpoints?.find((endpoint) => endpoint.id === sourceId);
  if (!source) return t("setup.speakerMissing");
  if (source.availability !== "ready" && !(source.availability === "missing" && source.canCreate)) return t("validation.setupSpeaker", { device: source.name });
  const output = snapshot.inventory.devices.find((device) => device.id === outputId && device.supportsOutput);
  if (!output || output.kind !== "physical" || isPipeWireDefaultAlias(outputId)) return t("validation.physicalPlayback");
  if (source.ancestry.recursivePaths.includes(outputId)) return t("validation.capturePath");
  for (const route of snapshot.routes) {
    if (!("physicalInput" in route.config.source) || route.state === "stopped") continue;
    const destination = route.config.destination;
    if ("virtualMicrophone" in destination && destination.virtualMicrophone === sourceId) return t("validation.stopMicrophone");
    if ("physicalOutput" in destination && destination.physicalOutput === outputId) return t("validation.separateHeadphones");
  }
  return null;
}
