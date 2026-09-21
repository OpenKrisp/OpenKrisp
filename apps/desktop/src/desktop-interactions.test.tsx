// @vitest-environment jsdom
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioDevice, EngineSnapshot, SettingsSnapshot, SaveSettingsRequest } from "./api";
import { defaultSettings, initialSettings } from "./preferences";
import { route, snapshot } from "./testFixtures";

const mocks = vi.hoisted(() => ({
  getEngineSnapshot: vi.fn(), refreshAudioDevices: vi.fn(), startRoute: vi.fn(), stopRoute: vi.fn(),
  updateRouteSettings: vi.fn(), setRouteBypass: vi.fn(), getSettings: vi.fn(), saveSettings: vi.fn(), subscribe: vi.fn(),
}));
vi.mock("./api", () => ({ api: mocks, subscribeToEngineEvents: mocks.subscribe }));
import { App } from "./App";
import { DiagnosticPanel } from "./DiagnosticPanel";

let root: Root;
let host: HTMLDivElement;
let currentSnapshot: EngineSnapshot;
let saved: SettingsSnapshot;
const microphone: AudioDevice = {
  id: "fake:input", name: "Test microphone", host: "coreaudio", kind: "physical", manufacturer: null, driver: null,
  supportsInput: true, supportsOutput: false, isDefaultInput: true, isDefaultOutput: false,
  defaultInputFormat: null, defaultOutputFormat: null,
};
const headphones: AudioDevice = { ...microphone, id: "fake:output", name: "Test headphones", supportsInput: false,
  supportsOutput: true, isDefaultInput: false, isDefaultOutput: true };

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.resetAllMocks();
  window.scrollTo = vi.fn();
  currentSnapshot = structuredClone(snapshot);
  currentSnapshot.inventory = { host: "coreaudio", devices: [microphone, headphones] };
  currentSnapshot.routing.capabilities.virtualMicrophone = true;
  currentSnapshot.routing.capabilities.systemLoopback = true;
  const endpoint = { id: "virtual:test", name: "Test endpoint", availability: "ready" as const, canCreate: false,
    playbackDeviceId: "virtual:output", captureDeviceId: "virtual:input", ancestry: { recursivePaths: [], complete: true }, instructions: [], setupUrl: "" };
  currentSnapshot.routing.endpoints = [endpoint];
  currentSnapshot.routing.loopbackEndpoints = [{ ...endpoint, id: "loopback:test" }];
  saved = { settings: defaultSettings(), revision: 0, savedRevision: 0, loadWarning: null, writeError: null };
  mocks.getEngineSnapshot.mockImplementation(async () => currentSnapshot);
  mocks.subscribe.mockResolvedValue(() => {});
  mocks.getSettings.mockImplementation(async () => structuredClone(saved));
  mocks.startRoute.mockResolvedValue({ routeId: "desktop-microphone" });
  mocks.saveSettings.mockImplementation(async (request: SaveSettingsRequest) => {
    saved = { settings: structuredClone(request.settings), revision: request.revision, savedRevision: request.revision, loadWarning: null, writeError: null };
    return structuredClone(saved);
  });
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
  const element = label?.control as HTMLInputElement | HTMLSelectElement | null;
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
  await change(field("Call microphone"), "virtual:test");
}

describe("real desktop controls", () => {
  it("keeps the daily screen focused and returns keyboard focus after Settings", async () => {
    await render();
    const mainPanel = host.querySelector('[aria-labelledby="microphone-title"]')!;
    const advanced = host.querySelector('[aria-labelledby="route-title"]')!;
    expect(mainPanel.closest("[hidden]")).toBeNull();
    expect(advanced.closest("[hidden]")).not.toBeNull();
    expect(host.querySelector<HTMLDetailsElement>(".incoming-panel")!.open).toBe(false);
    expect(host.querySelector(".call-setup")?.textContent).toContain("Test endpoint");
    await click(button("Settings"));
    expect(mainPanel.closest("[hidden]")).not.toBeNull();
    expect(advanced.closest("[hidden]")).toBeNull();
    expect(document.activeElement?.textContent).toBe("Settings");
    await change(field("Microphone target latency"), "55");
    await act(async () => document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(document.activeElement).toBe(button("Settings"));
    expect(mainPanel.closest("[hidden]")).toBeNull();
    await click(button("Settings"));
    expect(field("Microphone target latency").value).toBe("55");
  });

  it("saves the noise switch before Start and turns the primary action into Cancel then Stop", async () => {
    await render();
    const toggle = host.querySelector<HTMLButtonElement>('[aria-label="Microphone noise reduction"]')!;
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    await click(toggle);
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(saved.settings.microphone.mode).toBe("aligned_bypass");
    expect(mocks.setRouteBypass).not.toHaveBeenCalled();
    await click(button("Start microphone"));
    expect(mocks.startRoute).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ mode: "aligned_bypass" }));
    expect(button("Cancel start").disabled).toBe(false);
    await click(button("Cancel start"));
    expect(mocks.stopRoute).toHaveBeenCalledWith("desktop-microphone");
    await reconcile({ ...currentSnapshot, routes: [{ ...route, state: "recovering" }] });
    expect(button("Stop microphone").disabled).toBe(false);
    expect(toggle.disabled).toBe(true);
    expect(field("Microphone").matches(":disabled")).toBe(true);
    await click(button("Stop microphone"));
    expect(mocks.stopRoute).toHaveBeenLastCalledWith(route.config.id);
  });

  it("waits for confirmed runtime noise changes without saving temporary bypass", async () => {
    currentSnapshot.routes = [structuredClone(route)];
    await render();
    mocks.saveSettings.mockClear();
    const toggle = host.querySelector<HTMLButtonElement>('[aria-label="Microphone noise reduction"]')!;
    await click(toggle);
    expect(mocks.setRouteBypass).toHaveBeenCalledExactlyOnceWith(route.config.id, true);
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    expect(toggle.disabled).toBe(true);
    expect(button("Stop microphone").disabled).toBe(false);
    await reconcile({ ...currentSnapshot, routes: [{ ...route, bypassed: true, config: { ...route.config, processingMode: "aligned_bypass" } }] });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(toggle.disabled).toBe(false);
    expect(mocks.saveSettings).not.toHaveBeenCalled();
  });

  it("reenables a disabled denoiser before removing bypass and reports rejection", async () => {
    const disabled = { ...route, config: { ...route.config, denoise: { ...route.config.denoise, enabled: false } } };
    currentSnapshot.routes = [disabled];
    mocks.setRouteBypass.mockRejectedValueOnce({ code: "command_queue_full", message: "Audio is busy. Try again." });
    await render();
    const toggle = host.querySelector<HTMLButtonElement>('[aria-label="Microphone noise reduction"]')!;
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await click(toggle);
    expect(mocks.updateRouteSettings).toHaveBeenCalledExactlyOnceWith(route.config.id, { ...route.config.denoise, enabled: true });
    expect(mocks.setRouteBypass).toHaveBeenCalledExactlyOnceWith(route.config.id, false);
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(toggle.disabled).toBe(false);
    expect(host.textContent).toContain("Audio is busy. Try again.");
  });

  it("controls call audio independently and leaves music unprocessed by default", async () => {
    await render();
    const toggle = host.querySelector<HTMLButtonElement>('[aria-label="Call audio noise reduction"]')!;
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await click(toggle);
    await click(button("Start call audio"));
    expect(mocks.startRoute).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ routeId: "desktop-incoming", mode: "speech_denoise" }));
    expect(button("Cancel call audio start").disabled).toBe(false);
    await click(button("Cancel call audio start"));
    expect(mocks.stopRoute).toHaveBeenCalledExactlyOnceWith("desktop-incoming");
    expect(button("Start microphone").disabled).toBe(false);
  });

  it("clears the live level and avoids an On status after losing the connection", async () => {
    currentSnapshot.routes = [{ ...route, metrics: { ...route.metrics, inputPeak: .6 } }];
    await render();
    expect(host.querySelector<HTMLMeterElement>('[aria-label="Input level"]')!.value).toBe(.6);
    mocks.getEngineSnapshot.mockRejectedValueOnce({ code: "disconnected", message: "Reconnecting to audio." });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(host.querySelector<HTMLMeterElement>('[aria-label="Input level"]')!.value).toBe(0);
    expect(host.querySelector(".device-heading")?.textContent).toContain("Reconnecting…");
    expect(button("Stop microphone").disabled).toBe(true);
  });

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
    expect(status()).toContain("waiting for call audio");
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
    expect(button("Stop call audio").disabled).toBe(false);
    await click(button("Settings"));
    expect(button("Stop call audio", panel).closest("[hidden]")).toBeNull();
    expect(button("Stop call audio", panel).disabled).toBe(false);
    expect(field("Maximum reduction", host.querySelector(".settings")!).value).toBe("24");
  });

  it("selects dedicated endpoints and physical defaults, saves both routes and never starts them", async () => {
    await render();
    expect(field("Microphone").value).toBe("fake:input");
    expect(field("Call microphone").value).toBe("virtual:test");
    expect(field("Audio from").value).toBe("loopback:test");
    expect(field("Play through").value).toBe("fake:output");
    expect(saved.settings).toMatchObject({ microphone: { inputId: "fake:input", outputId: "virtual:test" },
      incoming: { inputId: "loopback:test", outputId: "fake:output", mode: "aligned_bypass" } });
    expect(mocks.startRoute).not.toHaveBeenCalled();
    expect(host.textContent).not.toContain("Profiles");
  });

  it("restores both routes and explicit AEC off after unmount/relaunch", async () => {
    await render();
    await click(field("Echo cancellation"));
    await change(field("Microphone target latency"), "65");
    await change(field("Processing"), "speech_denoise");
    await change(field("Incoming target latency"), "75");
    await act(async () => root.unmount());
    root = createRoot(host);
    mocks.saveSettings.mockClear();
    await render();
    expect((field("Echo cancellation") as HTMLInputElement).checked).toBe(false);
    expect(field("Microphone target latency").value).toBe("65");
    expect(field("Processing").value).toBe("speech_denoise");
    expect(field("Incoming target latency").value).toBe("75");
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    expect(mocks.startRoute).not.toHaveBeenCalled();
  });

  it("keeps missing devices selected through refresh and enables Start when they return", async () => {
    saved.settings.microphone.inputId = "missing:mic";
    saved.settings.incoming.outputId = "missing:output";
    await render();
    expect(field("Microphone").value).toBe("missing:mic");
    expect(field("Play through").value).toBe("missing:output");
    expect(button("Start microphone").disabled).toBe(true);
    expect(button("Start call audio").disabled).toBe(true);
    await reconcile(currentSnapshot);
    expect(field("Microphone").value).toBe("missing:mic");
    await reconcile({ ...currentSnapshot, inventory: { ...currentSnapshot.inventory,
      devices: [...currentSnapshot.inventory.devices, { ...microphone, id: "missing:mic" }, { ...headphones, id: "missing:output" }] } });
    expect(button("Start microphone").disabled).toBe(false);
    expect(button("Start call audio").disabled).toBe(false);
    expect(mocks.startRoute).not.toHaveBeenCalled();
  });

  it("waits for settings, then uses current running audio when it starts during the read", async () => {
    let resolve!: (settings: SettingsSnapshot) => void;
    mocks.getSettings.mockImplementationOnce(() => new Promise<SettingsSnapshot>((done) => { resolve = done; }));
    await render();
    expect(field("Microphone").matches(":disabled")).toBe(true);
    expect(button("Start microphone").disabled).toBe(true);
    await reconcile({ ...currentSnapshot, routes: [route] });
    expect(button("Stop microphone").disabled).toBe(false);
    await act(async () => resolve(structuredClone(saved)));
    expect(field("Headphones").value).toBe("fake:output");
    expect(field("Maximum reduction", host.querySelector(".settings")!).value).toBe("24");
    expect(mocks.startRoute).not.toHaveBeenCalled();
    expect(mocks.updateRouteSettings).not.toHaveBeenCalled();
  });

  it("retries a failed initial read without accepting edits or starting audio", async () => {
    mocks.getSettings.mockRejectedValueOnce({ code: "invalid_configuration", message: "Settings read failed." });
    await render();
    expect(host.textContent).toContain("Settings read failed.");
    expect(button("Start microphone").disabled).toBe(true);
    await click(button("Retry settings"));
    expect(field("Microphone").value).toBe("fake:input");
    expect(button("Start microphone").disabled).toBe(false);
    expect(mocks.startRoute).not.toHaveBeenCalled();
  });

  it("does not overwrite unreadable settings until the explicit replacement action", async () => {
    saved.loadWarning = { code: "invalid_configuration", message: "Saved settings are invalid.", details: null, suggestedAction: null };
    await render();
    await change(field("Microphone target latency"), "60");
    await change(field("Incoming target latency"), "70");
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    await click(button("Replace unreadable settings"));
    expect(mocks.saveSettings).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ replaceUnreadable: true,
      settings: expect.objectContaining({ microphone: expect.objectContaining({ targetLatencyMs: 60 }), incoming: expect.objectContaining({ targetLatencyMs: 70 }) }) }));
    expect(host.textContent).not.toContain("Saved settings are invalid.");
  });

  it("retains invalid numeric edits without poisoning the other route or losing later valid edits", async () => {
    await render();
    await change(field("Microphone target latency"), "");
    await click(field("Echo cancellation"));
    await change(field("Incoming target latency"), "80");
    expect(saved.settings.microphone.targetLatencyMs).toBe(40);
    expect(saved.settings.microphone.echoCancellation).toBe(true);
    expect(saved.settings.incoming.targetLatencyMs).toBe(80);
    expect(field("Microphone target latency").value).toBe("");
    await change(field("Microphone target latency"), "60");
    expect(saved.settings.microphone).toMatchObject({ targetLatencyMs: 60, echoCancellation: false });
    expect(saved.settings.incoming.targetLatencyMs).toBe(80);
  });

  it("keeps the latest pair through rapid edits and ignores stale save replies", async () => {
    await render();
    const completions: (() => void)[] = [];
    mocks.saveSettings.mockImplementation((request: SaveSettingsRequest) => new Promise<SettingsSnapshot>((resolve) => {
      const result = { ...saved, settings: structuredClone(request.settings), revision: request.revision, savedRevision: request.revision };
      completions.push(() => resolve(result));
    }));
    await change(field("Microphone target latency"), "60");
    await change(field("Incoming target latency"), "80");
    expect(mocks.saveSettings.mock.lastCall![0].settings).toMatchObject({ microphone: { targetLatencyMs: 60 }, incoming: { targetLatencyMs: 80 } });
    await act(async () => completions[0]!());
    expect(host.textContent).toContain("Saving settings…");
    expect(field("Incoming target latency").value).toBe("80");
    await act(async () => completions[1]!());
    expect(host.textContent).toContain("Settings saved automatically.");
  });

  it("polls accepted edits until disk confirms them without refilling controls", async () => {
    vi.useFakeTimers();
    await render();
    mocks.saveSettings.mockImplementation(async (request: SaveSettingsRequest) => {
      saved = { ...saved, settings: structuredClone(request.settings), revision: request.revision };
      return structuredClone(saved);
    });
    await change(field("Microphone target latency"), "60");
    expect(host.textContent).toContain("Saving settings…");
    saved.savedRevision = saved.revision;
    // Numeric editing may be temporarily invalid while the previous valid preference saves.
    await change(field("Microphone target latency"), "");
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(host.textContent).toContain("Settings saved automatically.");
    expect(field("Microphone target latency").value).toBe("");
  });

  it("retains choices on save failure, permits Stop, and retries the latest revision", async () => {
    currentSnapshot.routes = [route];
    await render();
    mocks.saveSettings.mockRejectedValueOnce({ code: "invalid_configuration", message: "Disk is full." });
    await change(field("Maximum reduction", host.querySelector(".settings")!), "18");
    expect(host.textContent).toContain("Disk is full.");
    expect(button("Stop microphone").disabled).toBe(false);
    expect(field("Maximum reduction", host.querySelector(".settings")!).value).toBe("18");
    await click(button("Retry settings"));
    expect(saved.settings.microphone.denoise.attenuationLimitDb).toBe(18);
    expect(host.textContent).not.toContain("Disk is full.");
  });

  it("does not persist runtime bypass or degrade user preferences when stopping", async () => {
    currentSnapshot.routes = [route];
    await render();
    mocks.saveSettings.mockClear();
    const bypassed = { ...route, bypassed: true, config: { ...route.config, processingMode: "aligned_bypass" as const }, metrics: { ...route.metrics, degraded: true } };
    await reconcile({ ...currentSnapshot, routes: [bypassed] });
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    await change(field("Maximum reduction", host.querySelector(".settings")!), "18");
    expect(saved.settings.microphone.mode).toBe("speech_denoise");
    await reconcile({ ...currentSnapshot, routes: [{ ...bypassed, state: "stopped" }] });
    expect(field("Microphone processing").value).toBe("speech_denoise");
  });

  it("hands a valid edit to Rust immediately even if the window unmounts before acknowledgement", async () => {
    await render();
    mocks.saveSettings.mockClear();
    mocks.saveSettings.mockImplementation(() => new Promise(() => {}));
    await change(field("Incoming target latency"), "90");
    await render(<div />);
    expect(mocks.saveSettings).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ settings: expect.objectContaining({ incoming: expect.objectContaining({ targetLatencyMs: 90 }) }) }));
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
