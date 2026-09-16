// @vitest-environment jsdom
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioDevice, AudioProfile, EngineSnapshot, ProfileCollection } from "./api";
import { defaultDraft, profileFromDraft } from "./drafts";
import { route, snapshot } from "./testFixtures";

const mocks = vi.hoisted(() => ({
  getEngineSnapshot: vi.fn(), refreshAudioDevices: vi.fn(), startRoute: vi.fn(), stopRoute: vi.fn(),
  updateRouteSettings: vi.fn(), setRouteBypass: vi.fn(), listProfiles: vi.fn(), saveProfile: vi.fn(),
  deleteProfile: vi.fn(), selectProfile: vi.fn(), subscribe: vi.fn(),
}));
vi.mock("./api", () => ({ api: mocks, subscribeToEngineEvents: mocks.subscribe }));
import { App } from "./App";
import { DiagnosticPanel } from "./DiagnosticPanel";
import { Profiles } from "./Profiles";

let root: Root;
let host: HTMLDivElement;
let currentSnapshot: EngineSnapshot;
let collection: ProfileCollection;
const microphone: AudioDevice = {
  id: "fake:input", name: "Test microphone", host: "coreaudio", kind: "physical", manufacturer: null, driver: null,
  supportsInput: true, supportsOutput: false, isDefaultInput: true, isDefaultOutput: false,
  defaultInputFormat: null, defaultOutputFormat: null,
};
const headphones: AudioDevice = { ...microphone, id: "fake:output", name: "Test headphones", supportsInput: false,
  supportsOutput: true, isDefaultInput: false, isDefaultOutput: true };
const defaultProfile = profileFromDraft({ ...defaultDraft("microphone"), destinationKind: "physical_output", inputId: "default", outputId: "default" }, "default", "Default");
const incomingProfile = profileFromDraft({ ...defaultDraft("incoming"), inputId: "loopback:test", outputId: "fake:output", targetLatencyMs: 70 }, "incoming", "Call playback");

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.resetAllMocks();
  currentSnapshot = structuredClone(snapshot);
  currentSnapshot.inventory = { host: "coreaudio", devices: [microphone, headphones] };
  currentSnapshot.routing.capabilities.virtualMicrophone = true;
  currentSnapshot.routing.capabilities.systemLoopback = true;
  const endpoint = { id: "virtual:test", name: "Test endpoint", availability: "ready" as const, canCreate: false,
    playbackDeviceId: "virtual:output", captureDeviceId: "virtual:input", ancestry: { recursivePaths: [], complete: true }, instructions: [], setupUrl: "" };
  currentSnapshot.routing.endpoints = [endpoint];
  currentSnapshot.routing.loopbackEndpoints = [{ ...endpoint, id: "loopback:test" }];
  collection = { schemaVersion: 3, selectedProfileId: "default", profiles: [structuredClone(defaultProfile), structuredClone(incomingProfile)], loadWarning: null };
  mocks.getEngineSnapshot.mockImplementation(async () => currentSnapshot);
  mocks.subscribe.mockResolvedValue(() => {});
  mocks.listProfiles.mockImplementation(async () => structuredClone(collection));
  mocks.startRoute.mockResolvedValue({ routeId: "desktop-microphone" });
  mocks.saveProfile.mockImplementation(async (profile: AudioProfile) => {
    collection.profiles = [...collection.profiles.filter((p) => p.id !== profile.id), structuredClone(profile)];
  });
  mocks.deleteProfile.mockImplementation(async (id: string) => {
    collection.profiles = collection.profiles.filter((p) => p.id !== id);
    if (collection.selectedProfileId === id) collection.selectedProfileId = collection.profiles[0]!.id;
  });
  mocks.selectProfile.mockImplementation(async (id: string) => { collection.selectedProfileId = id; });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); vi.useRealTimers(); });

async function render(node: ReactNode = <App />) { await act(async () => root.render(node)); }
function button(text: string, within: ParentNode = host) {
  const element = [...within.querySelectorAll("button")].find((button) => button.textContent === text);
  if (!element) throw new Error(`Missing button: ${text}`);
  return element;
}
function field(text: string, within: ParentNode = host) {
  const label = [...within.querySelectorAll("label")].find((label) => label.textContent?.startsWith(text));
  const element = label?.querySelector<HTMLInputElement | HTMLSelectElement>("input, select");
  if (!element) throw new Error(`Missing field: ${text}`);
  return element;
}
async function click(element: HTMLElement) { await act(async () => element.click()); }
async function change(element: HTMLInputElement | HTMLSelectElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(Object.getPrototypeOf(element), "value")!.set!.call(element, value);
    element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
async function reconcile(next: EngineSnapshot) {
  currentSnapshot = { ...next, revision: currentSnapshot.revision + 1 };
  await act(async () => window.dispatchEvent(new Event("focus")));
}
async function chooseMic() {
  await change(field("Microphone"), "fake:input");
  await change(field("Virtual endpoint"), "virtual:test");
}

describe("real desktop controls", () => {
  it("defaults echo cancellation on, submits an explicit off choice and locks it while running", async () => {
    await render(); await chooseMic();
    expect((field("Echo cancellation") as HTMLInputElement).checked).toBe(true);
    await click(field("Echo cancellation"));
    await click(button("Start microphone"));
    expect(mocks.startRoute).toHaveBeenCalledWith(expect.objectContaining({ echoCancellation: false }));
    const active = { ...route, config: { ...route.config, id: "desktop-microphone", echoCancellation: false } };
    await reconcile({ ...currentSnapshot, routes: [active] });
    expect(field("Echo cancellation").matches(":disabled")).toBe(true);
    expect((field("Echo cancellation") as HTMLInputElement).checked).toBe(false);
    expect(host.querySelector('[aria-label="Echo cancellation status"]')?.textContent).toBe("Echo cancellation is off.");
  });

  it("shows active AEC separately from denoise bypass and revokes the claim during route loss", async () => {
    const mic = { ...route, bypassed: true, config: { ...route.config, processingMode: "aligned_bypass" as const },
      echoCancellation: { status: "running" as const, processedFrames: 100, fallbackFrames: 10, resets: 0, nativeErrors: 0, maxProcessingUs: 100, missedDeadlines: 0 } };
    const incoming = { ...route, config: { ...route.config, id: "desktop-incoming", source: { systemLoopback: "loopback:test" }, echoCancellation: false } };
    currentSnapshot.routes = [mic, incoming];
    await render();
    const status = () => host.querySelector('[aria-label="Echo cancellation status"]')?.textContent ?? "";
    expect(status()).toContain("is active");
    await reconcile({ ...currentSnapshot, routes: [mic, { ...incoming, state: "stopped" }] });
    expect(status()).toContain("waiting for incoming audio");
    expect(status()).toContain("no echo protection");
    await reconcile({ ...currentSnapshot, routes: [{ ...mic, state: "recovering" }, incoming] });
    expect(status()).toContain("waiting for the microphone");
    await reconcile({ ...currentSnapshot, routes: [{ ...mic, echoCancellation: { ...mic.echoCancellation, status: "native_error" } }, incoming] });
    expect(status()).toContain("unavailable");
    expect(status()).toContain("without echo protection");
  });

  it("sends the edited route settings and blocks invalid settings before Start", async () => {
    await render(); await chooseMic();
    const settings = host.querySelector(".settings")!;
    await click(field("Limit maximum reduction", settings));
    await change(field("Maximum reduction", settings), "-1");
    expect(button("Start microphone").disabled).toBe(true);
    await change(field("Maximum reduction", settings), "12.5");
    await click(field("Additional post-filter", settings));
    await change(field("Microphone target latency"), "65");
    await change(field("Microphone processing"), "aligned_bypass");
    await click(button("Start microphone"));
    expect(mocks.startRoute).toHaveBeenCalledWith(expect.objectContaining({ sourceKind: "physical_input", destinationKind: "virtual_microphone",
      sourceDeviceId: "fake:input", destinationDeviceId: "virtual:test", targetLatencyMs: 65, mode: "aligned_bypass",
      denoise: { enabled: true, attenuationLimitDb: 12.5, postFilterEnabled: true } }));
    expect(mocks.updateRouteSettings).not.toHaveBeenCalled();
  });

  it("preserves dirty settings across metrics and bypass updates, and confirms live settings from a snapshot", async () => {
    currentSnapshot.routes = [structuredClone(route)];
    await render();
    const settings = host.querySelector(".settings")!;
    await change(field("Maximum reduction", settings), "18");
    await reconcile({ ...currentSnapshot, routes: [{ ...route, metrics: { ...route.metrics, inputPeak: 0.4 } }] });
    expect(field("Maximum reduction", settings).value).toBe("18");
    await reconcile({ ...currentSnapshot, routes: [{ ...route, bypassed: true, config: { ...route.config, processingMode: "aligned_bypass" } }] });
    expect(field("Maximum reduction", settings).value).toBe("18");
    await click(button("Apply noise reduction settings", settings));
    expect(mocks.updateRouteSettings).toHaveBeenCalledExactlyOnceWith(route.config.id, { ...route.config.denoise, attenuationLimitDb: 18 });
    expect(button("Applying settings…", settings).disabled).toBe(true);
    await reconcile(currentSnapshot);
    expect(settings.textContent).not.toContain("The engine accepted");
    const updated = structuredClone(currentSnapshot);
    updated.routes[0]!.config.denoise.attenuationLimitDb = 18;
    await reconcile(updated);
    expect(settings.textContent).toContain("The engine accepted these settings");
    expect(button("Apply noise reduction settings", settings).disabled).toBe(true);
  });

  it("reports rejected live updates and does not claim they were applied", async () => {
    currentSnapshot.routes = [structuredClone(route)];
    mocks.updateRouteSettings.mockRejectedValue({ code: "command_queue_full", message: "The engine is busy." });
    await render();
    const settings = host.querySelector(".settings")!;
    await change(field("Maximum reduction", settings), "18");
    await click(button("Apply noise reduction settings", settings));
    expect(settings.textContent).toContain("The engine is busy.");
    expect(settings.textContent).not.toContain("The engine accepted");
    expect(button("Apply noise reduction settings", settings).disabled).toBe(false);
  });

  it("updates only the incoming route's settings and keeps Stop available during an update", async () => {
    const incoming = { ...structuredClone(route), config: { ...route.config, id: "desktop-incoming", source: { systemLoopback: "loopback:test" } } };
    currentSnapshot.routes = [structuredClone(route), incoming];
    mocks.updateRouteSettings.mockImplementation(() => new Promise(() => {}));
    await render();
    const panel = host.querySelector('[aria-labelledby="incoming-title"]')!;
    await change(field("Maximum reduction", panel), "6");
    await click(button("Apply noise reduction settings", panel));
    expect(mocks.updateRouteSettings).toHaveBeenCalledExactlyOnceWith("desktop-incoming", { ...route.config.denoise, attenuationLimitDb: 6 });
    expect(button("Stop incoming audio").disabled).toBe(false);
    expect(field("Maximum reduction", host.querySelector(".settings")!).value).toBe("24");
  });

  it("loads missing device preferences without starting audio or choosing a fallback", async () => {
    collection.profiles.push({ ...defaultProfile, id: "missing", name: "Away", sourceDevicePreference: "missing:mic", destinationDevicePreference: "missing:output" });
    await render();
    await change(field("Saved profile"), "missing"); await click(button("Load profile"));
    expect(field("Microphone").value).toBe("missing:mic");
    expect(field("Headphones").value).toBe("missing:output");
    expect(button("Start microphone").disabled).toBe(true);
    expect(mocks.startRoute).not.toHaveBeenCalled();
    expect(mocks.selectProfile).toHaveBeenCalledWith("missing");
  });

  it("prevents loading onto an active microphone while allowing a stopped incoming profile", async () => {
    currentSnapshot.routes = [structuredClone(route)];
    await render();
    expect(button("Load profile").disabled).toBe(true);
    await change(field("Saved profile"), "incoming");
    expect(button("Load profile").disabled).toBe(false);
    await click(button("Load profile"));
    expect(field("Audio from").value).toBe("loopback:test");
    expect(field("Incoming target latency").value).toBe("70");
    expect(field("Processing").value).toBe("aligned_bypass");
    expect(mocks.startRoute).not.toHaveBeenCalled();
    expect(mocks.updateRouteSettings).not.toHaveBeenCalled();
  });

  it("saves and updates explicit route roles, then requires a separate deletion confirmation", async () => {
    await render(); await chooseMic();
    await change(field("Profile name"), "Work desk");
    await click(button("Save as new profile"));
    const saved = mocks.saveProfile.mock.calls[0]![0] as AudioProfile;
    expect(saved).toMatchObject({ name: "Work desk", sourceKind: "physical_input", destinationKind: "virtual_microphone", destinationDevicePreference: "virtual:test" });
    expect(saved.id).not.toBe("default");
    expect(field("Saved profile").value).toBe(saved.id);
    await change(field("Profile name"), "Renamed desk"); await click(button("Update selected profile"));
    expect(mocks.saveProfile.mock.calls[1]![0]).toMatchObject({ id: saved.id, name: "Renamed desk" });
    await click(button("Delete profile")); expect(mocks.deleteProfile).not.toHaveBeenCalled();
    await click(button("Cancel deletion")); expect(mocks.deleteProfile).not.toHaveBeenCalled();
    await click(button("Delete profile")); await click(button("Confirm delete"));
    expect(mocks.deleteProfile).toHaveBeenCalledExactlyOnceWith(saved.id);
    expect(host.textContent).not.toContain("Renamed desk");
    expect(mocks.startRoute).not.toHaveBeenCalled();
  });

  it("keeps a failed save visible without changing the selected profile or losing edits", async () => {
    mocks.saveProfile.mockRejectedValue({ code: "invalid_configuration", message: "Profiles could not be saved on this device." });
    await render(); await chooseMic(); await change(field("Profile name"), "Keep this edit");
    await click(button("Save as new profile"));
    expect(field("Profile name").value).toBe("Keep this edit");
    expect(field("Saved profile").value).toBe("default");
    expect(host.textContent).toContain("Profiles could not be saved");
    expect(host.textContent).not.toContain("Profile saved.");
  });

  it("does not overwrite an unreadable config when loading fallback preferences", async () => {
    collection.loadWarning = { code: "invalid_configuration", message: "Saved profiles are invalid.", details: null, suggestedAction: null };
    await render(); await click(button("Load profile"));
    expect(mocks.selectProfile).not.toHaveBeenCalled();
    expect(mocks.saveProfile).not.toHaveBeenCalled();
    expect(field("Microphone").value).toBe("fake:input");
    expect(field("Headphones").value).toBe("fake:output");
    expect(button("Delete profile").disabled).toBe(true);
    expect(button("Replace unreadable profiles and save")).toBeDefined();
  });

  it("ignores an initial profile read that arrives after a newer refresh", async () => {
    let resolveOld!: (collection: ProfileCollection) => void;
    mocks.listProfiles.mockImplementationOnce(() => new Promise<ProfileCollection>((resolve) => { resolveOld = resolve; }));
    await render();
    await click(button("Refresh profiles"));
    await act(async () => resolveOld({ ...collection, profiles: [{ ...defaultProfile, name: "Stale name" }] }));
    expect(host.textContent).not.toContain("Stale name");
    expect(field("Profile name").value).toBe("Default");
  });

  it("does not load into a route which starts while profile selection is awaiting persistence", async () => {
    let resolveSelection!: () => void;
    mocks.selectProfile.mockImplementationOnce(() => new Promise<void>((resolve) => { resolveSelection = resolve; }));
    const onLoad = vi.fn();
    const targets = { microphone: { draft: defaultDraft("microphone"), locked: false }, incoming: { draft: defaultDraft("incoming"), locked: false } };
    await render(<Profiles snapshot={currentSnapshot} connected targets={targets} onLoad={onLoad} onBusy={() => {}} />);
    await click(button("Load profile"));
    await render(<Profiles snapshot={currentSnapshot} connected targets={{ ...targets, microphone: { ...targets.microphone, locked: true } }} onLoad={onLoad} onBusy={() => {}} />);
    await act(async () => resolveSelection());
    expect(onLoad).not.toHaveBeenCalled();
    expect(host.textContent).toContain("The route changed while loading");
  });

  it("provides a frozen sanitized report and a manual copy fallback", async () => {
    await render(<DiagnosticPanel snapshot={{ ...currentSnapshot, routes: [route] }} connected={false} />);
    expect(host.textContent).toContain("Disconnected: showing the last received snapshot");
    expect(host.textContent).toContain("Inference p95"); expect(host.textContent).toContain("Callback violations");
    await click(button("Prepare report"));
    const text = host.querySelector("textarea")!;
    const original = text.value;
    await render(<DiagnosticPanel snapshot={{ ...currentSnapshot, routes: [] }} connected />);
    expect(text.value).toBe(original);
    await click(button("Copy report"));
    expect(host.textContent).toContain("The report is selected");
    expect(text.selectionStart).toBe(0); expect(text.selectionEnd).toBe(text.value.length);
    await click(button("Refresh report"));
    expect(JSON.parse(text.value).routes).toEqual([]);
  });
});
