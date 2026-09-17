import { draftFromProfile, profileFromDraft, defaultDraft } from "./drafts";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { EngineSnapshot } from "./api";
import { IncomingAudio } from "./IncomingAudio";
import { incomingSelectionError, incomingStartRequest } from "./incoming";
import { routeSelectionError } from "./routing";
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
    const markup = renderToStaticMarkup(<IncomingAudio draft={defaultDraft("incoming")} onDraft={() => {}} profilesBusy={false} onPending={() => {}} snapshot={ready} connected />);
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
    expect(renderToStaticMarkup(<IncomingAudio draft={defaultDraft("incoming")} onDraft={() => {}} profilesBusy={false} onPending={() => {}} snapshot={snapshot} connected />)).toContain("unavailable");
  });
  it("renders incoming state independently when the microphone is first", () => {
    const concurrent = structuredClone(ready);
    concurrent.routes = [route, { ...route, state: "recovering", config: { ...route.config, id: "incoming", source: { systemLoopback: "macos:blackhole-2ch" } } }];
    expect(renderToStaticMarkup(<IncomingAudio draft={defaultDraft("incoming")} onDraft={() => {}} profilesBusy={false} onPending={() => {}} snapshot={concurrent} connected />)).toContain("Route: recovering");
  });
});


describe("separate macOS call devices", () => {
  function dualSnapshot(): EngineSnapshot {
    const dual = structuredClone(ready);
    dual.routing.capabilities.virtualMicrophone = true;
    const microphoneBus = dual.routing.loopbackEndpoints![0]!;
    dual.routing.endpoints = [structuredClone(microphoneBus)];
    dual.routing.loopbackEndpoints!.unshift({ ...microphoneBus,
      id: "macos:blackhole-16ch", name: "BlackHole 16ch",
      captureDeviceId: "blackhole16", playbackDeviceId: "blackhole16",
      ancestry: { recursivePaths: ["blackhole16"], complete: true },
      instructions: ["Select BlackHole 16ch as the call speaker, channels 1–2."],
    });
    dual.inventory.devices.push({ ...dual.inventory.devices[0]!, id: "mic", supportsInput: true });
    return dual;
  }

  it("allows either start order and retains same-bus protection through recovery and Stop", () => {
    const dual = dualSnapshot();
    const selection = { inputId: "mic", outputId: "macos:blackhole-2ch", destinationKind: "virtual_microphone" as const };
    for (const state of ["starting", "running", "recovering", "stopping", "failed"] as const) {
      dual.routes = [{ ...route, state, config: { ...route.config,
        source: { physicalInput: "mic" }, destination: { virtualMicrophone: selection.outputId },
      } }];
      expect(incomingSelectionError(dual, "macos:blackhole-16ch", "headphones")).toBeNull();
      expect(incomingSelectionError(dual, "macos:blackhole-2ch", "headphones")).toContain("Choose BlackHole 16ch");
      dual.routes = [{ ...route, state, config: { ...route.config,
        source: { systemLoopback: "macos:blackhole-16ch" }, destination: { physicalOutput: "headphones" },
      } }];
      expect(routeSelectionError(dual, selection)).toBeNull();
      dual.routes[0]!.config.source = { systemLoopback: "macos:blackhole-2ch" };
      expect(routeSelectionError(dual, selection)).toContain("switch it to BlackHole 16ch");
    }
    dual.routes[0]!.state = "stopped";
    expect(routeSelectionError(dual, selection)).toBeNull();
  });

  it("persists the selected bus and exposes missing-driver setup without substituting 2ch", () => {
    const dual = dualSnapshot();
    const draft = { ...defaultDraft("incoming"), inputId: "macos:blackhole-16ch", outputId: "headphones" };
    const profile = profileFromDraft(draft, "call", "Call audio");
    dual.routing.loopbackEndpoints![0]!.availability = "missing";
    const restored = draftFromProfile(profile, dual);
    expect(restored.inputId).toBe("macos:blackhole-16ch");
    expect(incomingSelectionError(dual, restored.inputId, restored.outputId)).toContain("Set up BlackHole 16ch");
    const html = renderToStaticMarkup(<IncomingAudio draft={restored} onDraft={() => {}} profilesBusy={false} onPending={() => {}} snapshot={dual} connected />);
    expect(html).toContain('value="macos:blackhole-16ch" selected=""');
    expect(html).toContain("Set up BlackHole 16ch");
    expect(incomingStartRequest(restored.inputId, restored.outputId).sourceDeviceId).toBe("macos:blackhole-16ch");
  });
});
