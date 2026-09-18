import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { AudioDevice, EngineSnapshot } from "./api";
import { EndpointSetup } from "./EndpointSetup";
import { isPipeWireDefaultAlias, routeCanStop, routeIsActive, routeSelectionError, routeToStop, selectionFromRoute, startRequest } from "./routing";
import { route, snapshot } from "./testFixtures";

const microphone: AudioDevice = {
  id: "coreaudio:mic", name: "Microphone", host: "coreaudio", kind: "physical",
  manufacturer: null, driver: null, supportsInput: true, supportsOutput: false,
  isDefaultInput: true, isDefaultOutput: false, defaultInputFormat: null, defaultOutputFormat: null,
};
const ready: EngineSnapshot = {
  ...snapshot,
  inventory: { host: "coreaudio", devices: [microphone] },
  routing: {
    capabilities: { ...snapshot.routing.capabilities, virtualMicrophone: true, requiresExternalDriver: true },
    endpoints: [{
      id: "macos:openkrisp-microphone", name: "OpenKrisp Microphone", canCreate: false, availability: "ready",
      playbackDeviceId: "coreaudio:OpenKrispMicrophone_UID", captureDeviceId: "coreaudio:OpenKrispMicrophone_UID",
      ancestry: { recursivePaths: ["coreaudio:OpenKrispMicrophone_UID"], complete: true },
      instructions: ["Select OpenKrisp Microphone as the microphone in your receiving app."],
      setupUrl: "https://github.com/saeta-eth/OpenKrisp/blob/main/docs/INSTALLATION.md",
    }],
  },
};
const selection = { inputId: microphone.id, outputId: "macos:openkrisp-microphone", destinationKind: "virtual_microphone" as const };

describe("virtual microphone route controls", () => {
  it("permits Linux creation only for a missing creatable endpoint", () => {
    const linux = structuredClone(ready);
    const endpoint = linux.routing.endpoints[0]!;
    Object.assign(endpoint, { id: "linux:pipewire", name: "OpenKrisp Microphone", canCreate: true, availability: "missing", playbackDeviceId: null, captureDeviceId: null });
    const linuxSelection = { ...selection, outputId: endpoint.id };
    expect(routeSelectionError(linux, linuxSelection)).toBeNull();
    expect(renderToStaticMarkup(<EndpointSetup routing={linux.routing} />)).toContain("Created when you start");
    endpoint.availability = "invalid";
    expect(routeSelectionError(linux, linuxSelection)).toContain("Set up OpenKrisp Microphone");
    endpoint.availability = "unsupported";
    expect(routeSelectionError(linux, linuxSelection)).not.toBeNull();
  });

  it("builds an explicit virtual request and restores it after reload", () => {
    expect(routeSelectionError(ready, selection)).toBeNull();
    expect(startRequest(selection)).toMatchObject({
      sourceDeviceId: microphone.id, destinationDeviceId: "macos:openkrisp-microphone", destinationKind: "virtual_microphone",
    });
    expect(selectionFromRoute({ ...route, config: { ...route.config,
      source: { physicalInput: microphone.id }, destination: { virtualMicrophone: selection.outputId },
    } })).toEqual(selection);
    expect(selectionFromRoute(route).destinationKind).toBe("physical_output");
  });

  it("blocks start when an endpoint disappears without selecting another output", () => {
    const missing = structuredClone(ready);
    missing.routing.endpoints[0]!.availability = "missing";
    expect(routeSelectionError(missing, selection)).toContain("Set up OpenKrisp Microphone");
    expect(selection.outputId).toBe("macos:openkrisp-microphone");
    missing.inventory.devices = [];
    expect(routeSelectionError(missing, selection)).toContain("available microphone");
    expect(routeSelectionError(ready, { ...selection, outputId: "unknown-endpoint" })).not.toBeNull();
  });

  it("rejects unsafe sources and unsupported platforms", () => {
    for (const kind of ["aggregate", "virtual", "unknown"] as const) {
      const unsafe = { ...ready, inventory: { ...ready.inventory, devices: [{ ...microphone, kind }] } };
      expect(routeSelectionError(unsafe, selection)).toContain("physical microphone");
    }
    const unsupported = { ...ready, routing: snapshot.routing };
    expect(routeSelectionError(unsupported, selection)).toContain("unavailable in this build");
  });

  it("keeps stop available throughout startup and recovery and permits restart after failure", () => {
    for (const state of ["starting", "running", "reconfiguring", "recovering", "stopping"] as const) {
      expect(routeIsActive({ ...route, state })).toBe(true);
    }
    expect(routeIsActive({ ...route, state: "failed" })).toBe(false);
    expect(routeIsActive({ ...route, state: "stopped" })).toBe(false);
    expect(routeIsActive(undefined)).toBe(false);
    expect(routeToStop({ ...route, state: "stopped" }, true)).toBe(startRequest(selection).routeId);
    expect(routeToStop(route, false)).toBe(route.config.id);
    expect(routeToStop(undefined, false)).toBeUndefined();
  });

  it("allows Stop to clean up a failed route while also permitting a new Start", () => {
    const failed = { ...route, state: "failed" as const };
    expect(routeIsActive(failed)).toBe(false);
    expect(routeCanStop(failed, false)).toBe(true);
    expect(routeToStop(failed, false)).toBe(failed.config.id);
    for (const state of ["starting", "running", "recovering", "reconfiguring"] as const) {
      expect(routeCanStop({ ...route, state }, false)).toBe(true);
    }
    expect(routeCanStop({ ...route, state: "stopping" }, false)).toBe(false);
    expect(routeCanStop({ ...route, state: "stopped" }, false)).toBe(false);
    expect(routeCanStop(undefined, false)).toBe(false);
    expect(routeCanStop(undefined, true)).toBe(true);
    expect(routeCanStop({ ...route, state: "stopped" }, true)).toBe(true);
  });

  it("rejects moving PipeWire aliases even when restored from a saved route", () => {
    const devices = [
      { ...microphone, id: "pipewire:mic", host: "pipewire" },
      { ...microphone, id: "pipewire:headphones", host: "pipewire", supportsInput: false, supportsOutput: true },
      { ...microphone, id: "pipewire:input_default", host: "pipewire", kind: "unknown" as const },
      { ...microphone, id: "pipewire:output_default", host: "pipewire", kind: "unknown" as const, supportsInput: false, supportsOutput: true },
    ];
    const linux = { ...ready, inventory: { host: "pipewire", devices } };
    const concrete = { inputId: "pipewire:mic", outputId: "pipewire:headphones", destinationKind: "physical_output" as const };
    expect(routeSelectionError(linux, concrete)).toBeNull();
    expect(devices.filter((device) => !isPipeWireDefaultAlias(device.id)).map((device) => device.id))
      .toEqual(["pipewire:mic", "pipewire:headphones"]);
    for (const aliases of [{ inputId: "pipewire:input_default" }, { outputId: "pipewire:output_default" }]) {
      const restored = selectionFromRoute({ ...route, config: { ...route.config,
        source: { physicalInput: aliases.inputId ?? concrete.inputId },
        destination: { physicalOutput: aliases.outputId ?? concrete.outputId },
      } });
      expect(routeSelectionError(linux, restored)).toContain("specific microphone and headphone output");
    }
  });

  it("renders actionable setup, missing, invalid and unsupported states", () => {
    for (const [availability, text] of [["ready", "Ready"], ["missing", "Installation needed"], ["invalid", "Unsupported device configuration"]] as const) {
      const routing = structuredClone(ready.routing);
      routing.endpoints[0]!.availability = availability;
      const html = renderToStaticMarkup(<EndpointSetup routing={routing} />);
      expect(html).toContain(text);
      expect(html).toContain("https://github.com/saeta-eth/OpenKrisp/blob/main/docs/INSTALLATION.md");
      expect(html).toContain("receiving app");
      expect(html).toContain("headphones");
    }
    expect(renderToStaticMarkup(<EndpointSetup routing={snapshot.routing} />)).toContain("unavailable in this build");
  });
});

describe("named microphone setup", () => {
  it("rejects obsolete profile endpoints instead of mapping them to a new device", () => {
    for (const outputId of ["macos:blackhole-2ch", "macos:blackhole-16ch"]) {
      expect(routeSelectionError(ready, { ...selection, outputId })).toContain("Choose a virtual microphone");
    }
    expect(ready.routing.endpoints).toHaveLength(1);
    expect(renderToStaticMarkup(<EndpointSetup routing={ready.routing} />)).toContain("Select OpenKrisp Microphone as the microphone");
  });
});
