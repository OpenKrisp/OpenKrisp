import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { EngineSnapshot } from "./api";
import { IncomingAudio } from "./IncomingAudio";
import { incomingSelectionError, incomingStartRequest } from "./incoming";
import { route, snapshot } from "./testFixtures";

const ready: EngineSnapshot = {
  ...snapshot,
  inventory: { host: "coreaudio", devices: [{
    id: "headphones", name: "Headphones", host: "coreaudio", kind: "physical", manufacturer: null, driver: null,
    supportsInput: false, supportsOutput: true, isDefaultInput: false, isDefaultOutput: true,
    defaultInputFormat: null, defaultOutputFormat: null,
  }] },
  routing: {
    ...snapshot.routing,
    capabilities: { ...snapshot.routing.capabilities, systemLoopback: true },
    loopbackEndpoints: [{ id: "macos:blackhole-2ch", name: "BlackHole 2ch", availability: "ready", canCreate: false,
      playbackDeviceId: "blackhole", captureDeviceId: "blackhole", ancestry: { recursivePaths: ["blackhole"], complete: true },
      instructions: ["Select BlackHole as your media application's output."], setupUrl: "" }],
  },
};

describe("incoming audio", () => {
  it("defaults music to bypass and makes speech denoise explicit", () => {
    expect(incomingStartRequest("macos:blackhole-2ch", "headphones")).toMatchObject({ sourceKind: "system_loopback", mode: "aligned_bypass" });
    expect(incomingStartRequest("macos:blackhole-2ch", "headphones", "speech_denoise").mode).toBe("speech_denoise");
    const markup = renderToStaticMarkup(<IncomingAudio snapshot={ready} connected />);
    expect(markup).toContain('value="aligned_bypass" selected=""');
    expect(markup).toContain("keep music and other audio in bypass");
  });
  it("accepts isolated physical rendering and rejects recursion or unverifiable outputs", () => {
    expect(incomingSelectionError(ready, "macos:blackhole-2ch", "headphones")).toBeNull();
    for (const kind of ["aggregate", "virtual", "unknown"] as const) {
      const invalid = structuredClone(ready); invalid.inventory.devices[0]!.kind = kind;
      expect(incomingSelectionError(invalid, "macos:blackhole-2ch", "headphones")).not.toBeNull();
    }
    const recursive = structuredClone(ready);
    recursive.routing.loopbackEndpoints![0]!.ancestry.recursivePaths.push("headphones");
    expect(incomingSelectionError(recursive, "macos:blackhole-2ch", "headphones")).toContain("capture path");
  });
  it("blocks a shared BlackHole microphone path and competing render output", () => {
    const busy = structuredClone(ready);
    busy.routes = [{ ...route, config: { ...route.config, destination: { virtualMicrophone: "macos:blackhole-2ch" } } }];
    expect(incomingSelectionError(busy, "macos:blackhole-2ch", "headphones")).toContain("Stop the virtual microphone");
    busy.routes[0]!.config.destination = { physicalOutput: "headphones" };
    expect(incomingSelectionError(busy, "macos:blackhole-2ch", "headphones")).toContain("separate headphones");
    busy.routes[0]!.state = "stopped";
    expect(incomingSelectionError(busy, "macos:blackhole-2ch", "headphones")).toBeNull();
  });
  it("allows missing creatable endpoints and handles older snapshots", () => {
    const linux = structuredClone(ready);
    linux.routing.loopbackEndpoints![0]!.availability = "missing";
    expect(incomingSelectionError(linux, "macos:blackhole-2ch", "headphones")).toContain("Set up");
    linux.routing.loopbackEndpoints![0]!.canCreate = true;
    expect(incomingSelectionError(linux, "macos:blackhole-2ch", "headphones")).toBeNull();
    expect(incomingSelectionError(snapshot, "", "")).toContain("unavailable");
    expect(renderToStaticMarkup(<IncomingAudio snapshot={snapshot} connected />)).toContain("unavailable");
  });
  it("renders incoming state independently when the microphone is first", () => {
    const concurrent = structuredClone(ready);
    concurrent.routes = [route, { ...route, state: "recovering", config: { ...route.config, id: "incoming", source: { systemLoopback: "macos:blackhole-2ch" } } }];
    expect(renderToStaticMarkup(<IncomingAudio snapshot={concurrent} connected />)).toContain("Route: recovering");
  });
});
