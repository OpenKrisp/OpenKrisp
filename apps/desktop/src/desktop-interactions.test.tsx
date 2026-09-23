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
import { SupportDetails } from "./SupportDetails";

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
  Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
  currentSnapshot = structuredClone(snapshot);
  currentSnapshot.inventory = { host: "coreaudio", devices: [microphone, headphones, { ...microphone, id: "fake:input2", name: "USB microphone", isDefaultInput: false }, { ...headphones, id: "fake:output2", name: "USB headphones", isDefaultOutput: false }] };
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
  const element = [...within.querySelectorAll("button")].find((button) => button.textContent === text || button.getAttribute("aria-label") === text);
  if (!element) throw new Error(`Missing button: ${text}`);
  return element;
}
function callAudioButton(text: string) { return button(text, host.querySelector('[aria-label="Your speakers controls"]')!); }
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

describe("real desktop controls", () => {
  it("removes engineering controls and restores keyboard focus and device choices after Help", async () => {
    await render();
    await change(field("Microphone"), "fake:input2");
    const mainPanel = host.querySelector('[aria-labelledby="microphone-title"]')!;
    expect(mainPanel.querySelector('[role="switch"]')).toBeNull();
    const incomingPanel = host.querySelector('[aria-labelledby="speakers-title"]')!;
    expect(incomingPanel.closest("details, [hidden]")).toBeNull();
    expect(callAudioButton("Turn on OpenKrisp").closest("details, [hidden]")).toBeNull();
    expect(host.querySelector(".call-setup")?.textContent).toContain("Test endpoint");
    await click(button("Help"));
    expect(mainPanel.closest("[hidden]")).not.toBeNull();
    expect(incomingPanel.closest("[hidden]")).not.toBeNull();
    expect(host.querySelector('[aria-labelledby="speaker-help-title"]')?.textContent).toContain("Test endpoint");
    expect(button("Refresh devices").closest("[hidden]")).toBeNull();
    expect(button("Copy support details").closest("[hidden]")).toBeNull();
    for (const retired of ["Settings saved automatically.", "Listen to original sound", "Sound comparison", "Advanced audio", "Target latency", "Maximum reduction", "post-filter", "Use microphone for", "Inference p95", "Processing"]) {
      expect(host.textContent).not.toContain(retired);
    }
    expect(host.querySelector('input[type="number"], input[type="checkbox"], textarea')).toBeNull();
    expect(document.activeElement?.textContent).toBe("Help");
    await act(async () => document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(document.activeElement).toBe(button("Help"));
    expect(mainPanel.closest("[hidden]")).toBeNull();
    expect(incomingPanel.closest("[hidden]")).toBeNull();
    expect(field("Microphone").value).toBe("fake:input2");
  });

  it("opens actionable setup help and recovers after the audio device is installed", async () => {
    currentSnapshot.routing.endpoints[0]!.availability = "missing";
    await render();
    await click(button("Get setup help", host.querySelector(".microphone-panel")!));
    expect(host.querySelector('.connection-guide')!.closest("[hidden]")).toBeNull();
    expect(host.textContent).toContain("Run the OpenKrisp installer");
    await click(button("Refresh devices"));
    expect(mocks.refreshAudioDevices).toHaveBeenCalledExactlyOnceWith();
    currentSnapshot.routing.endpoints[0]!.availability = "ready";
    await reconcile(currentSnapshot);
    await click(button("Back"));
    expect(button("Turn on OpenKrisp").disabled).toBe(false);
  });

  it("keeps pending call audio stoppable in Help", async () => {
    await render();
    await click(callAudioButton("Turn on OpenKrisp"));
    await click(button("Help"));
    const panel = host.querySelector('[aria-labelledby="incoming-title"]')!;
    expect(button("Cancel start", panel).closest("[hidden]")).toBeNull();
    await click(button("Cancel start", panel));
    expect(mocks.stopRoute).toHaveBeenCalledExactlyOnceWith("desktop-incoming");
    expect(panel.closest("[hidden]")).not.toBeNull();
  });

  it.each(["microphone", "speakers"] as const)("keeps %s startup visibly pending and cancellable while Help stays responsive", async (kind) => {
    vi.useFakeTimers();
    const microphone = kind === "microphone";
    const startingRoute = { ...route, state: "starting" as const, config: { ...route.config,
      id: microphone ? "desktop-microphone" : "desktop-incoming",
      source: microphone ? { physicalInput: "fake:input" } : { systemLoopback: "loopback:test" },
    } };
    await render();
    const panel = host.querySelector(microphone ? ".microphone-panel" : ".incoming-panel")!;
    await click(button("Turn on OpenKrisp", panel));
    expect(button("Cancel start", panel).disabled).toBe(false);
    expect(panel.querySelector(".status-spinner")).not.toBeNull();
    expect(panel.textContent).toContain("Preparing noise reduction.");

    // The engine has accepted Start, but loading the model is still in progress.
    await reconcile({ ...currentSnapshot, routes: [startingRoute] });
    await act(async () => { await vi.advanceTimersByTimeAsync(3_000); });
    expect(button("Cancel start", panel).disabled).toBe(false);
    expect(panel.querySelector(".status-spinner")).not.toBeNull();
    await click(button("Help"));
    const helpStatus = host.querySelector(microphone ? '[aria-label="Microphone status"]' : '[aria-labelledby="incoming-title"]')!;
    expect(document.activeElement?.textContent).toBe("Help");
    expect(button("Cancel start", helpStatus).disabled).toBe(false);
    await click(button("Cancel start", helpStatus));
    expect(mocks.stopRoute).toHaveBeenCalledExactlyOnceWith(startingRoute.config.id);
    await reconcile({ ...currentSnapshot, routes: [{ ...startingRoute, state: "stopped" }] });
    await click(button("Back"));
    expect(panel.querySelector(".status-spinner")).toBeNull();

    await click(button("Turn on OpenKrisp", panel));
    await reconcile({ ...currentSnapshot, routes: [{ ...startingRoute, state: "running" }] });
    expect(button("Turn off OpenKrisp", panel).disabled).toBe(false);
    expect(panel.querySelector(".status-spinner")).toBeNull();
    expect(panel.textContent).not.toContain("Preparing noise reduction.");
  });

  it("turns on noise reduction even with an older bypass preference and permits cancellation and stopping", async () => {
    saved.settings.microphone.mode = "aligned_bypass";
    saved.settings.microphone.denoise.enabled = false;
    await render();
    const flow = host.querySelector('[aria-label="Microphone audio flow"]')!;
    expect(flow.textContent).toContain("Reduces the noise");
    expect(flow.textContent).toContain("OpenKrisp microphone");
    expect(mocks.setRouteBypass).not.toHaveBeenCalled();
    await click(button("Turn on OpenKrisp"));
    expect(mocks.startRoute).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ mode: "speech_denoise", denoise: expect.objectContaining({ enabled: true }) }));
    expect(saved.settings.microphone.mode).toBe("speech_denoise");
    expect(saved.settings.microphone.denoise.enabled).toBe(true);
    expect(button("Cancel start").disabled).toBe(false);
    await click(button("Help"));
    const status = host.querySelector('[aria-label="Microphone status"]')!;
    expect(button("Cancel start", status).closest("[hidden]")).toBeNull();
    await click(button("Cancel start", status));
    expect(mocks.stopRoute).toHaveBeenCalledWith("desktop-microphone");
    await reconcile({ ...currentSnapshot, routes: [{ ...route, state: "recovering" }] });
    expect(button("Turn off OpenKrisp").disabled).toBe(false);
    expect(field("Microphone").matches(":disabled")).toBe(true);
    await click(button("Turn off OpenKrisp"));
    expect(mocks.stopRoute).toHaveBeenLastCalledWith(route.config.id);
  });

  it("preserves a running call-audio bypass until stopped and enables cleanup on the next start", async () => {
    const incoming = { ...route, bypassed: true, config: { ...route.config, id: "desktop-incoming", source: { systemLoopback: "loopback:test" }, processingMode: "aligned_bypass" as const } };
    currentSnapshot.routes = [incoming];
    await render();
    const flow = host.querySelector('[aria-label="Your speakers flow"]')!;
    expect(flow.textContent).toContain("Original sound");
    expect(mocks.setRouteBypass).not.toHaveBeenCalled();
    expect(mocks.updateRouteSettings).not.toHaveBeenCalled();
    expect(callAudioButton("Turn off OpenKrisp").disabled).toBe(false);
    await click(callAudioButton("Turn off OpenKrisp"));
    expect(mocks.stopRoute).toHaveBeenCalledExactlyOnceWith(incoming.config.id);
    await reconcile({ ...currentSnapshot, routes: [{ ...incoming, state: "stopped" }] });
    expect(flow.textContent).toContain("Reduces the noise");
    await click(callAudioButton("Turn on OpenKrisp"));
    expect(mocks.startRoute).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ mode: "speech_denoise", denoise: expect.objectContaining({ enabled: true }) }));
  });

  it("allows retrying a rejected call-audio start without starting the microphone", async () => {
    mocks.startRoute.mockRejectedValueOnce({ code: "command_queue_full", message: "Audio is busy. Try again." });
    await render();
    await click(callAudioButton("Turn on OpenKrisp"));
    expect(host.textContent).toContain("Audio is busy. Try again.");
    expect(callAudioButton("Turn on OpenKrisp").disabled).toBe(false);
    await click(callAudioButton("Turn on OpenKrisp"));
    expect(mocks.startRoute).toHaveBeenCalledTimes(2);
    expect(mocks.startRoute.mock.calls.every(([request]) => request.routeId === "desktop-incoming")).toBe(true);
    expect(callAudioButton("Cancel start").disabled).toBe(false);
  });

  it("starts call cleanup with one action despite old bypass preferences and permits cancellation", async () => {
    saved.settings.incoming.mode = "aligned_bypass";
    saved.settings.incoming.denoise.enabled = false;
    await render();
    expect(host.querySelector('[role="switch"]')).toBeNull();
    const flow = host.querySelector('[aria-label="Your speakers flow"]')!;
    expect(flow.textContent).toContain("Reduces the noise");
    await click(callAudioButton("Turn on OpenKrisp"));
    expect(mocks.startRoute).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ routeId: "desktop-incoming", mode: "speech_denoise", denoise: expect.objectContaining({ enabled: true }) }));
    expect(flow.textContent).toContain("Waiting for audio");
    expect(saved.settings.incoming.mode).toBe("speech_denoise");
    expect(callAudioButton("Cancel start").disabled).toBe(false);
    await click(callAudioButton("Cancel start"));
    expect(mocks.stopRoute).toHaveBeenCalledExactlyOnceWith("desktop-incoming");
    expect(button("Turn on OpenKrisp").disabled).toBe(false);
  });

  it("clears the live level and avoids an On status after losing the connection", async () => {
    currentSnapshot.routes = [{ ...route, metrics: { ...route.metrics, inputPeak: .01 } }];
    await render();
    expect(host.querySelector<HTMLMeterElement>('[aria-label="Input level"]')!.value).toBeCloseTo(1 / 3);
    mocks.getEngineSnapshot.mockRejectedValueOnce({ code: "disconnected", message: "Reconnecting to audio." });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(host.querySelector<HTMLMeterElement>('[aria-label="Input level"]')!.value).toBe(0);
    expect(host.querySelector(".microphone-panel .audio-panel-heading")?.textContent).toContain("Reconnecting…");
    const flow = host.querySelector('[aria-label="Microphone audio flow"]')!;
    expect(flow.textContent).toContain("Waiting for audio");
    expect(flow.classList.contains("is-running")).toBe(false);
    expect(button("Turn off OpenKrisp").disabled).toBe(true);
  });

  it("updates both meters on metrics events without waiting for the fallback poll", async () => {
    vi.useFakeTimers();
    const microphoneRoute = { ...route, metrics: { ...route.metrics, inputPeak: .01, outputPeak: .8 } };
    const speakerRoute = { ...route, metrics: { ...route.metrics, inputPeak: .9, outputPeak: .1 },
      config: { ...route.config, id: "desktop-incoming", source: { systemLoopback: "loopback:test" } } };
    currentSnapshot.routes = [microphoneRoute, speakerRoute];
    await render();
    const input = host.querySelector<HTMLMeterElement>('[aria-label="Input level"]')!;
    const playback = host.querySelector<HTMLMeterElement>('[aria-label="Playback level"]')!;
    expect(input.value).toBeCloseTo(1 / 3);
    expect(playback.value).toBeCloseTo(2 / 3);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
      currentSnapshot = { ...currentSnapshot, revision: currentSnapshot.revision + 1,
        routes: [{ ...microphoneRoute, metrics: { ...microphoneRoute.metrics, inputPeak: .1 } },
          { ...speakerRoute, metrics: { ...speakerRoute.metrics, outputPeak: .01 } }] };
      const onEvent = mocks.subscribe.mock.calls[0]![0];
      onEvent({ type: "metrics", revision: currentSnapshot.revision, routeId: microphoneRoute.config.id,
        metrics: currentSnapshot.routes[0]!.metrics });
    });
    expect(input.value).toBeCloseTo(2 / 3);
    expect(playback.value).toBeCloseTo(1 / 3);

    await reconcile({ ...currentSnapshot, routes: currentSnapshot.routes.map((running) => ({ ...running, state: "stopped" })) });
    expect(input.value).toBe(0);
    expect(playback.value).toBe(0);
  });

  it("shows headphones in test mode and explains when noise reduction is paused", async () => {
    currentSnapshot.routes = [{ ...route, metrics: { ...route.metrics, degraded: true } }];
    await render();
    const flow = host.querySelector('[aria-label="Microphone audio flow"]')!;
    expect(flow.textContent).toContain("Your headphones");
    expect(flow.textContent).not.toContain("For your call app");
    expect(flow.textContent).toContain("Noise reduction paused");
    expect(host.querySelector('[aria-labelledby="call-setup-title"]')).toBeNull();
    await reconcile({ ...currentSnapshot, routes: [route] });
    expect(flow.textContent).toContain("Reducing the noise");
  });

  it("replaces legacy manual tuning and monitoring with the built-in call defaults on Start", async () => {
    saved.settings.microphone = { ...saved.settings.microphone, destinationKind: "physical_output", outputId: "fake:output",
      targetLatencyMs: 90, echoCancellation: false, denoise: { enabled: false, attenuationLimitDb: 12, postFilterEnabled: true } };
    await render();
    await click(button("Turn on OpenKrisp"));
    expect(mocks.startRoute).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      sourceDeviceId: "fake:input", destinationDeviceId: "virtual:test", destinationKind: "virtual_microphone",
      targetLatencyMs: 40, echoCancellation: true, mode: "speech_denoise",
      denoise: { enabled: true, attenuationLimitDb: null, postFilterEnabled: false },
    }));
  });

  it("reflects confirmed call cleanup, degradation and connection loss in the flow and level", async () => {
    const incoming = { ...route, metrics: { ...route.metrics, outputPeak: .1 }, config: { ...route.config, id: "desktop-incoming", source: { systemLoopback: "loopback:test" }, echoCancellation: false } };
    currentSnapshot.routes = [route, incoming];
    await render();
    const flow = host.querySelector('[aria-label="Your speakers flow"]')!;
    const level = host.querySelector<HTMLMeterElement>('[aria-label="Playback level"]')!;
    expect(flow.textContent).toContain("Reducing the noise");
    expect(level.value).toBeCloseTo(2 / 3);
    await reconcile({ ...currentSnapshot, routes: [route, { ...incoming, metrics: { ...incoming.metrics, degraded: true } }] });
    expect(flow.textContent).toContain("Noise reduction paused");
    expect(callAudioButton("Turn off OpenKrisp").disabled).toBe(false);
    await reconcile({ ...currentSnapshot, routes: [route, { ...incoming, state: "recovering" }] });
    expect(flow.textContent).toContain("Waiting for audio");
    expect(flow.classList.contains("is-running")).toBe(false);
    expect(level.value).toBe(0);
    mocks.getEngineSnapshot.mockRejectedValueOnce({ code: "disconnected", message: "Reconnecting to audio." });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(flow.textContent).toContain("Waiting for audio");
    expect(callAudioButton("Turn off OpenKrisp").disabled).toBe(true);
  });

  it("uses automatic call-audio routing and tuning for speech cleanup", async () => {
    saved.settings.incoming = { ...saved.settings.incoming, inputId: "old:endpoint", mode: "speech_denoise",
      targetLatencyMs: 95, denoise: { enabled: false, attenuationLimitDb: 12, postFilterEnabled: true } };
    await render();
    await click(callAudioButton("Turn on OpenKrisp"));
    expect(mocks.startRoute).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      sourceDeviceId: "loopback:test", destinationDeviceId: "fake:output", destinationKind: "physical_output",
      mode: "speech_denoise", targetLatencyMs: 40, echoCancellation: false,
      denoise: { enabled: true, attenuationLimitDb: null, postFilterEnabled: false },
    }));
  });



  it("keeps both Stop controls available in Help while call audio is recovering", async () => {
    const incoming = { ...structuredClone(route), state: "recovering" as const, config: { ...route.config, id: "desktop-incoming", source: { systemLoopback: "loopback:test" } } };
    currentSnapshot.routes = [structuredClone(route), incoming];
    await render();
    await click(button("Help"));
    const panel = host.querySelector('[aria-labelledby="incoming-title"]')!;
    expect(button("Turn off OpenKrisp", panel).closest("[hidden]")).toBeNull();
    expect(button("Turn off OpenKrisp", panel).disabled).toBe(false);
    expect(button("Turn off OpenKrisp", host.querySelector('[aria-label="Microphone status"]')!).closest("[hidden]")).toBeNull();
    await click(button("Turn off OpenKrisp", panel));
    expect(mocks.stopRoute).toHaveBeenCalledExactlyOnceWith("desktop-incoming");
  });

  it("selects dedicated endpoints and physical defaults, saves both routes and never starts them", async () => {
    await render();
    expect(field("Microphone").value).toBe("fake:input");
    expect(field("Play through").value).toBe("fake:output");
    expect(saved.settings).toMatchObject({ microphone: { inputId: "fake:input", outputId: "virtual:test" },
      incoming: { inputId: "loopback:test", outputId: "fake:output", mode: "speech_denoise" } });
    expect(mocks.startRoute).not.toHaveBeenCalled();
    expect(host.textContent).not.toContain("Profiles");
  });

  it("restores physical device choices and call cleanup after relaunch without starting audio", async () => {
    await render();
    await change(field("Microphone"), "fake:input2");
    await change(field("Play through"), "fake:output2");
    await act(async () => root.unmount());
    root = createRoot(host);
    mocks.saveSettings.mockClear();
    await render();
    expect(field("Microphone").value).toBe("fake:input2");
    expect(field("Play through").value).toBe("fake:output2");
    expect(host.querySelector('[aria-label="Your speakers flow"]')!.textContent).toContain("Reduces the noise");
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    expect(mocks.startRoute).not.toHaveBeenCalled();
  });

  it("keeps missing devices selected through refresh and enables Start when they return", async () => {
    saved.settings.microphone.inputId = "missing:mic";
    saved.settings.incoming.outputId = "missing:output";
    await render();
    expect(field("Microphone").value).toBe("missing:mic");
    expect(field("Play through").value).toBe("missing:output");
    expect(button("Turn on OpenKrisp").disabled).toBe(true);
    expect(callAudioButton("Turn on OpenKrisp").disabled).toBe(true);
    await reconcile(currentSnapshot);
    expect(field("Microphone").value).toBe("missing:mic");
    await reconcile({ ...currentSnapshot, inventory: { ...currentSnapshot.inventory,
      devices: [...currentSnapshot.inventory.devices, { ...microphone, id: "missing:mic" }, { ...headphones, id: "missing:output" }] } });
    expect(button("Turn on OpenKrisp").disabled).toBe(false);
    expect(callAudioButton("Turn on OpenKrisp").disabled).toBe(false);
    expect(mocks.startRoute).not.toHaveBeenCalled();
  });

  it("waits for settings, then uses current running audio when it starts during the read", async () => {
    let resolve!: (settings: SettingsSnapshot) => void;
    mocks.getSettings.mockImplementationOnce(() => new Promise<SettingsSnapshot>((done) => { resolve = done; }));
    await render();
    expect(field("Microphone").matches(":disabled")).toBe(true);
    expect(button("Turn on OpenKrisp").disabled).toBe(true);
    await reconcile({ ...currentSnapshot, routes: [route] });
    expect(button("Turn off OpenKrisp").disabled).toBe(false);
    await act(async () => resolve(structuredClone(saved)));
    expect(field("Microphone").value).toBe("fake:input");
    expect(field("Microphone").matches(":disabled")).toBe(true);
    expect(host.querySelector('[aria-label="Microphone audio flow"]')!.textContent).toContain("Your headphones");
    expect(mocks.startRoute).not.toHaveBeenCalled();
    expect(mocks.updateRouteSettings).not.toHaveBeenCalled();
  });

  it("retries a failed initial read without accepting edits or starting audio", async () => {
    mocks.getSettings.mockRejectedValueOnce({ code: "invalid_configuration", message: "Settings read failed." });
    await render();
    expect(host.textContent).toContain("Settings read failed.");
    expect(button("Turn on OpenKrisp").disabled).toBe(true);
    await click(button("Retry settings"));
    expect(field("Microphone").value).toBe("fake:input");
    expect(button("Turn on OpenKrisp").disabled).toBe(false);
    expect(mocks.startRoute).not.toHaveBeenCalled();
  });

  it("does not overwrite unreadable settings until the explicit replacement action", async () => {
    saved.loadWarning = { code: "invalid_configuration", message: "Saved settings are invalid.", details: null, suggestedAction: null };
    await render();
    await change(field("Microphone"), "fake:input2");
    await change(field("Play through"), "fake:output2");
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    await click(button("Replace unreadable settings"));
    expect(mocks.saveSettings).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ replaceUnreadable: true,
      settings: expect.objectContaining({ microphone: expect.objectContaining({ inputId: "fake:input2" }), incoming: expect.objectContaining({ outputId: "fake:output2" }) }) }));
    expect(host.textContent).not.toContain("Saved settings are invalid.");
  });


  it("keeps the latest pair through rapid edits and ignores stale save replies", async () => {
    vi.useFakeTimers();
    await render();
    mocks.getSettings.mockClear();
    const completions: (() => void)[] = [];
    mocks.saveSettings.mockImplementation((request: SaveSettingsRequest) => new Promise<SettingsSnapshot>((resolve) => {
      const result = { ...saved, settings: structuredClone(request.settings), revision: request.revision, savedRevision: request.revision };
      completions.push(() => resolve(result));
    }));
    await change(field("Microphone"), "fake:input2");
    await change(field("Play through"), "fake:output2");
    expect(mocks.saveSettings.mock.lastCall![0].settings).toMatchObject({ microphone: { inputId: "fake:input2" }, incoming: { outputId: "fake:output2" } });
    await act(async () => completions[0]!());
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(mocks.getSettings).toHaveBeenCalledTimes(1);
    expect(field("Play through").value).toBe("fake:output2");
    await act(async () => completions[1]!());
    mocks.getSettings.mockClear();
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(mocks.getSettings).not.toHaveBeenCalled();
  });

  it("polls accepted edits until disk confirms them without refilling controls", async () => {
    vi.useFakeTimers();
    await render();
    mocks.saveSettings.mockImplementation(async (request: SaveSettingsRequest) => {
      saved = { ...saved, settings: structuredClone(request.settings), revision: request.revision };
      return structuredClone(saved);
    });
    await change(field("Microphone"), "fake:input2");
    mocks.getSettings.mockClear();
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(mocks.getSettings).toHaveBeenCalledTimes(1);
    saved.savedRevision = saved.revision;
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(mocks.getSettings).toHaveBeenCalledTimes(2);
    mocks.getSettings.mockClear();
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(mocks.getSettings).not.toHaveBeenCalled();
    expect(field("Microphone").value).toBe("fake:input2");
  });

  it("retains choices on save failure, permits Stop, and retries the latest revision", async () => {
    currentSnapshot.routes = [route];
    await render();
    mocks.saveSettings.mockRejectedValueOnce({ code: "invalid_configuration", message: "Disk is full." });
    await change(field("Play through"), "fake:output2");
    expect(host.textContent).toContain("Disk is full.");
    expect(button("Turn off OpenKrisp").disabled).toBe(false);
    expect(field("Play through").value).toBe("fake:output2");
    await click(button("Retry settings"));
    expect(saved.settings.incoming.outputId).toBe("fake:output2");
    expect(host.textContent).not.toContain("Disk is full.");
  });

  it("does not persist runtime bypass or degrade user preferences when stopping", async () => {
    currentSnapshot.routes = [route];
    await render();
    mocks.saveSettings.mockClear();
    const bypassed = { ...route, bypassed: true, config: { ...route.config, processingMode: "aligned_bypass" as const }, metrics: { ...route.metrics, degraded: true } };
    await reconcile({ ...currentSnapshot, routes: [bypassed] });
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    await change(field("Play through"), "fake:output2");
    expect(saved.settings.microphone.mode).toBe("speech_denoise");
    await reconcile({ ...currentSnapshot, routes: [{ ...bypassed, state: "stopped" }] });
    expect(host.querySelector('[aria-label="Microphone audio flow"]')!.textContent).toContain("Reduces the noise");
    await click(button("Turn on OpenKrisp"));
    expect(mocks.startRoute).toHaveBeenCalledWith(expect.objectContaining({ mode: "speech_denoise",
      denoise: expect.objectContaining({ enabled: true, attenuationLimitDb: null }) }));
  });

  it("hands a valid edit to Rust immediately even if the window unmounts before acknowledgement", async () => {
    await render();
    mocks.saveSettings.mockClear();
    mocks.saveSettings.mockImplementation(() => new Promise(() => {}));
    await change(field("Play through"), "fake:output2");
    await render(<div />);
    expect(mocks.saveSettings).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ settings: expect.objectContaining({ incoming: expect.objectContaining({ outputId: "fake:output2" }) }) }));
  });

  it("copies sanitized support details without rendering technical counters", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    await render(<SupportDetails snapshot={{ ...currentSnapshot, routes: [route] }} connected={false} />);
    expect(host.querySelector('textarea')).toBeNull();
    expect(host.textContent).not.toContain("Inference p95");
    expect(writeText).not.toHaveBeenCalled();
    await click(button("Copy support details"));
    expect(writeText).toHaveBeenCalledExactlyOnceWith(expect.any(String));
    expect(JSON.parse(writeText.mock.calls[0]![0]).routes).toHaveLength(1);
    expect(writeText.mock.calls[0]![0]).not.toContain("fake:input");
    expect(host.querySelector('textarea')).toBeNull();
    expect(host.textContent).toContain("Support details copied.");
  });

  it("offers a frozen manual copy fallback when the clipboard is unavailable", async () => {
    await render(<SupportDetails snapshot={{ ...currentSnapshot, routes: [route] }} connected={false} />);
    await click(button("Copy support details"));
    const text = host.querySelector("textarea")!;
    const original = text.value;
    expect(text.selectionStart).toBe(0); expect(text.selectionEnd).toBe(text.value.length);
    await render(<SupportDetails snapshot={{ ...currentSnapshot, routes: [] }} connected />);
    expect(text.value).toBe(original);
    await click(button("Copy support details"));
    expect(JSON.parse(text.value).routes).toEqual([]);
  });

});
