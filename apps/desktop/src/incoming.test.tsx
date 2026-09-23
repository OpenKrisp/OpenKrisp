import { defaultSettings, initialSettings, preferencesFromDraft } from "./preferences";
import { defaultDraft } from "./drafts";
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
    loopbackEndpoints: [{ id: "macos:openkrisp-speaker", name: "OpenKrisp Speaker", availability: "ready", canCreate: false,
      playbackDeviceId: "coreaudio:OpenKrispSpeaker_UID", captureDeviceId: "coreaudio:OpenKrispSpeaker_UID", ancestry: { recursivePaths: ["coreaudio:OpenKrispSpeaker_UID"], complete: true },
      instructions: ["Select OpenKrisp Speaker as your media application's output."], setupUrl: "" }],
  },
};

describe("incoming audio", () => {
  it("keeps bypass available to the API while desktop Call audio offers speech cleanup", () => {
    expect(incomingStartRequest("macos:openkrisp-speaker", "headphones")).toMatchObject({ sourceKind: "system_loopback", mode: "aligned_bypass" });
    expect(incomingStartRequest("macos:openkrisp-speaker", "headphones", "speech_denoise").mode).toBe("speech_denoise");
    const markup = renderToStaticMarkup(<IncomingAudio draft={defaultDraft("incoming")} onDraft={() => {}} settingsReady snapshot={ready} connected />);
    expect(markup).toContain('aria-label="Your speakers flow"');
    expect(markup).toContain("Reduces the noise");
    expect(markup).not.toContain('role="switch"');
    expect(markup).toContain("Use your usual headphones or speakers for music.");
  });
  it("accepts isolated physical rendering and rejects recursion or unverifiable outputs", () => {
    expect(incomingSelectionError(ready, "macos:openkrisp-speaker", "headphones")).toBeNull();
    for (const kind of ["aggregate", "virtual", "unknown"] as const) {
      const invalid = structuredClone(ready); invalid.inventory.devices[0]!.kind = kind;
      expect(incomingSelectionError(invalid, "macos:openkrisp-speaker", "headphones")).not.toBeNull();
    }
    const recursive = structuredClone(ready);
    recursive.routing.loopbackEndpoints![0]!.ancestry.recursivePaths.push("headphones");
    expect(incomingSelectionError(recursive, "macos:openkrisp-speaker", "headphones")).toContain("capture path");
  });
  it("blocks a shared virtual path and competing render output", () => {
    const busy = structuredClone(ready);
    busy.routes = [{ ...route, config: { ...route.config, destination: { virtualMicrophone: "macos:openkrisp-speaker" } } }];
    expect(incomingSelectionError(busy, "macos:openkrisp-speaker", "headphones")).toContain("Stop the virtual microphone");
    busy.routes[0]!.config.destination = { physicalOutput: "headphones" };
    expect(incomingSelectionError(busy, "macos:openkrisp-speaker", "headphones")).toContain("separate headphones");
    busy.routes[0]!.state = "stopped";
    expect(incomingSelectionError(busy, "macos:openkrisp-speaker", "headphones")).toBeNull();
  });
  it("allows missing creatable endpoints and handles older snapshots", () => {
    const linux = structuredClone(ready);
    linux.routing.loopbackEndpoints![0]!.availability = "missing";
    expect(incomingSelectionError(linux, "macos:openkrisp-speaker", "headphones")).toContain("Set up");
    linux.routing.loopbackEndpoints![0]!.canCreate = true;
    expect(incomingSelectionError(linux, "macos:openkrisp-speaker", "headphones")).toBeNull();
    expect(incomingSelectionError(snapshot, "", "")).toContain("unavailable");
    expect(renderToStaticMarkup(<IncomingAudio draft={defaultDraft("incoming")} onDraft={() => {}} settingsReady snapshot={snapshot} connected />)).toContain("unavailable");
  });
  it("renders incoming state independently when the microphone is first", () => {
    const concurrent = structuredClone(ready);
    concurrent.routes = [route, { ...route, state: "recovering", config: { ...route.config, id: "incoming", source: { systemLoopback: "macos:openkrisp-speaker" } } }];
    expect(renderToStaticMarkup(<IncomingAudio draft={defaultDraft("incoming")} onDraft={() => {}} settingsReady snapshot={concurrent} connected />)).toContain("Reconnecting audio…");
  });
});


describe("separate macOS call devices", () => {
  function dualSnapshot(): EngineSnapshot {
    const dual = structuredClone(ready);
    dual.routing.capabilities.virtualMicrophone = true;
    dual.routing.endpoints = [{ ...dual.routing.loopbackEndpoints![0]!,
      id: "macos:openkrisp-microphone", name: "OpenKrisp Microphone",
      captureDeviceId: "coreaudio:OpenKrispMicrophone_UID", playbackDeviceId: "coreaudio:OpenKrispMicrophone_UID",
      ancestry: { recursivePaths: ["coreaudio:OpenKrispMicrophone_UID"], complete: true },
      instructions: ["Select OpenKrisp Microphone as the call microphone."],
    }];
    dual.inventory.devices.push({ ...dual.inventory.devices[0]!, id: "mic", supportsInput: true });
    return dual;
  }

  it("allows either start order and retains same-bus protection through recovery and Stop", () => {
    const dual = dualSnapshot();
    const selection = { inputId: "mic", outputId: "macos:openkrisp-microphone", destinationKind: "virtual_microphone" as const };
    for (const state of ["starting", "running", "recovering", "stopping", "failed"] as const) {
      dual.routes = [{ ...route, state, config: { ...route.config,
        source: { physicalInput: "mic" }, destination: { virtualMicrophone: selection.outputId },
      } }];
      expect(incomingSelectionError(dual, "macos:openkrisp-speaker", "headphones")).toBeNull();
      expect(incomingSelectionError(dual, "macos:openkrisp-microphone", "headphones")).toContain("speaker wasn’t found");
      dual.routes = [{ ...route, state, config: { ...route.config,
        source: { systemLoopback: "macos:openkrisp-speaker" }, destination: { physicalOutput: "headphones" },
      } }];
      expect(routeSelectionError(dual, selection)).toBeNull();
      dual.routes[0]!.config.source = { systemLoopback: "macos:openkrisp-microphone" };
      expect(routeSelectionError(dual, selection)).toContain("Stop incoming audio");
    }
    dual.routes[0]!.state = "stopped";
    expect(routeSelectionError(dual, selection)).toBeNull();
  });

  it("persists the selected bus and exposes missing-driver setup without substituting the microphone", () => {
    const dual = dualSnapshot();
    const draft = { ...defaultDraft("incoming"), inputId: "macos:openkrisp-speaker", outputId: "headphones" };
    const saved = preferencesFromDraft(draft);
    dual.routing.loopbackEndpoints![0]!.availability = "missing";
    const restored = { ...initialSettings({ ...defaultSettings(), incoming: saved }, dual).incoming, kind: "incoming" as const };
    expect(restored.inputId).toBe("macos:openkrisp-speaker");
    expect(incomingSelectionError(dual, restored.inputId, restored.outputId)).toContain("Set up OpenKrisp Speaker");
    const html = renderToStaticMarkup(<IncomingAudio draft={restored} onDraft={() => {}} settingsReady snapshot={dual} connected />);
    expect(html).toContain('<strong>OpenKrisp Speaker</strong>');
    expect(html).not.toContain('Audio from<select');
    expect(html).toContain("Set up OpenKrisp Speaker");
    expect(incomingStartRequest(restored.inputId, restored.outputId).sourceDeviceId).toBe("macos:openkrisp-speaker");
  });
});

describe("OpenKrisp device names", () => {
  it("shows the native speaker name and keeps its identity across saved settings", () => {
    const named = structuredClone(ready);
    named.routes = [{ ...route, config: { ...route.config,
      source: { physicalInput: "mic" }, destination: { virtualMicrophone: "macos:openkrisp-microphone" },
    } }];
    const draft = { ...defaultDraft("incoming"), inputId: "macos:openkrisp-speaker", outputId: "headphones" };
    expect(incomingSelectionError(named, draft.inputId, draft.outputId)).toBeNull();
    const restored = { ...initialSettings({ ...defaultSettings(), incoming: preferencesFromDraft(draft) }, named).incoming, kind: "incoming" as const };
    expect(restored.inputId).toBe("macos:openkrisp-speaker");
    const html = renderToStaticMarkup(<IncomingAudio draft={restored} onDraft={() => {}} settingsReady snapshot={named} connected />);
    expect(html).toContain('<strong>OpenKrisp Speaker</strong>');
    expect(html).not.toContain('Audio from<select');
    expect(html).toContain("OpenKrisp Speaker");
    for (const inputId of ["macos:blackhole-2ch", "macos:blackhole-16ch"]) {
      const obsolete = { ...draft, inputId };
      const saved = { ...initialSettings({ ...defaultSettings(), incoming: preferencesFromDraft(obsolete) }, named).incoming, kind: "incoming" as const };
      expect(saved.inputId).toBe("macos:openkrisp-speaker");
      expect(incomingSelectionError(named, saved.inputId, saved.outputId)).toBeNull();
    }
    expect(named.routing.loopbackEndpoints).toHaveLength(1);
    named.routing.loopbackEndpoints![0]!.availability = "missing";
    expect(incomingSelectionError(named, restored.inputId, restored.outputId)).toContain("Set up OpenKrisp Speaker");
  });
});
