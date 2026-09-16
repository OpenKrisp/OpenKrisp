import { beforeEach, describe, expect, it, vi } from "vitest";

import type { EngineEvent } from "./api";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriMocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauriMocks.listen }));

import { api, subscribeToEngineEvents } from "./api";

const denoise = {
  enabled: true,
  attenuationLimitDb: 24,
  postFilterEnabled: true,
};

describe("desktop IPC client", () => {
  beforeEach(() => {
    tauriMocks.invoke.mockReset();
    tauriMocks.invoke.mockResolvedValue(undefined);
    tauriMocks.listen.mockReset();
  });

  it("sends virtual endpoint identifiers with an explicit destination kind", async () => {
    const request = {
      routeId: "virtual-mic", sourceDeviceId: "coreaudio:mic",
      destinationDeviceId: "macos:blackhole-2ch", destinationKind: "virtual_microphone" as const,
      mode: "speech_denoise" as const, targetLatencyMs: 40, denoise,
    };
    await api.startRoute(request);
    expect(tauriMocks.invoke).toHaveBeenCalledWith("start_route", { request });
  });

  it("uses the exact command names and argument envelopes", async () => {
    const startRequest = {
      routeId: "desktop-monitor",
      sourceDeviceId: "fake:input",
      destinationDeviceId: "fake:output",
      mode: "speech_denoise" as const,
      targetLatencyMs: 40,
      denoise,
    };
    const profile = {
      id: "office",
      name: "Office",
      sourceKind: "physical_input" as const,
      destinationKind: "physical_output" as const,
      processingMode: "speech_denoise" as const,
      echoCancellation: true,
      sourceDevicePreference: "fake:input",
      destinationDevicePreference: "fake:output",
      targetLatencyMs: 40,
      denoise,
    };

    await api.listAudioDevices();
    await api.refreshAudioDevices();
    await api.startRoute(startRequest);
    await api.stopRoute("desktop-monitor");
    await api.updateRouteSettings("desktop-monitor", denoise);
    await api.setRouteBypass("desktop-monitor", true);
    await api.getEngineSnapshot();
    await api.listProfiles();
    await api.saveProfile(profile);
    await api.deleteProfile("office");
    await api.selectProfile("office");

    expect(tauriMocks.invoke.mock.calls).toEqual([
      ["list_audio_devices"],
      ["refresh_audio_devices"],
      ["start_route", { request: startRequest }],
      ["stop_route", { routeId: "desktop-monitor" }],
      ["update_route_settings", { request: { routeId: "desktop-monitor", denoise } }],
      ["set_route_bypass", { request: { routeId: "desktop-monitor", bypassed: true } }],
      ["get_engine_snapshot"],
      ["list_profiles"],
      ["save_profile", { profile }],
      ["delete_profile", { profileId: "office" }],
      ["select_profile", { profileId: "office" }],
    ]);
  });

  it("subscribes to every low-rate topic and releases every listener", async () => {
    type EventCallback = (event: { payload: EngineEvent }) => void;
    const callbacks = new Map<string, EventCallback>();
    const unlisteners = Array.from({ length: 4 }, () => vi.fn<() => void>());
    tauriMocks.listen.mockImplementation(
      async (topic: string, callback: EventCallback): Promise<() => void> => {
        callbacks.set(topic, callback);
        return unlisteners[callbacks.size - 1] ?? vi.fn<() => void>();
      },
    );
    const observed: EngineEvent[] = [];

    const unsubscribe = await subscribeToEngineEvents((event) => observed.push(event));
    expect([...callbacks.keys()]).toEqual([
      "audio://state-changed",
      "audio://devices-changed",
      "audio://metrics",
      "audio://fault",
    ]);

    const stateEvent: EngineEvent = {
      revision: 2,
      type: "state_changed",
      routeId: "desktop-monitor",
      state: "running",
    };
    callbacks.get("audio://state-changed")?.({ payload: stateEvent });
    expect(observed).toEqual([stateEvent]);

    unsubscribe();
    for (const unlisten of unlisteners) expect(unlisten).toHaveBeenCalledOnce();
  });

  it("cleans up partial subscriptions when listener setup fails", async () => {
    const unlisteners = [vi.fn<() => void>(), vi.fn<() => void>()];
    tauriMocks.listen
      .mockResolvedValueOnce(unlisteners[0]!)
      .mockResolvedValueOnce(unlisteners[1]!)
      .mockRejectedValueOnce(new Error("listener rejected"));

    await expect(subscribeToEngineEvents(vi.fn())).rejects.toThrow("listener rejected");
    expect(unlisteners[0]!).toHaveBeenCalledOnce();
    expect(unlisteners[1]!).toHaveBeenCalledOnce();
  });
});
