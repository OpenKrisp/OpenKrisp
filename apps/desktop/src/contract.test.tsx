// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import contract from "../../../tests/fixtures/desktop-contract";
import type { ApiError, EngineEvent, EngineSnapshot, SettingsSnapshot } from "./api";
import type { UpdateStatus } from "./updates";
import { audioStatus } from "./AudioControls";
import { normalizeApiError, initialDesktopState, reduceDesktopState } from "./state";
import { UpdateNotice } from "./UpdateNotice";

type ReadonlyDeep<T> = T extends object ? { readonly [P in keyof T]: ReadonlyDeep<T[P]> } : T;
// Compile-time checks apply directly to the literal serialized by Rust; no shape cast.
const snapshot: ReadonlyDeep<EngineSnapshot> = contract.snapshot;
const settings: ReadonlyDeep<SettingsSnapshot> = contract.settings;
const error: ReadonlyDeep<ApiError> = contract.error;
const events: ReadonlyDeep<EngineEvent>[] = contract.events.map(({ payload }) => payload);
const updates: readonly ReadonlyDeep<UpdateStatus>[] = contract.updates;

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
import { api, subscribeToEngineEvents } from "./api";

describe("shared Rust and TypeScript desktop contract", () => {
  it("produces the same envelopes that the real Rust IPC handler accepts", async () => {
    mocks.invoke.mockReset();
    await api.listAudioDevices();
    await api.refreshAudioDevices();
    await api.startRoute(contract.calls[2][1].request);
    await api.updateRouteSettings(contract.calls[3][1].request.routeId, contract.calls[3][1].request.denoise);
    await api.setRouteBypass(contract.calls[4][1].request.routeId, contract.calls[4][1].request.bypassed);
    await api.stopRoute(contract.calls[5][1].routeId);
    await api.getEngineSnapshot();
    await api.getSettings();
    expect(mocks.invoke.mock.calls).toEqual(contract.calls);
  });

  it("reconciles Rust snapshots and errors with production UI consumers", () => {
    // JSON delivery naturally removes fixture immutability at the bridge boundary.
    const received: EngineSnapshot = JSON.parse(JSON.stringify(snapshot));
    const state = reduceDesktopState(initialDesktopState, { type: "reconcile", snapshot: received });
    expect(state.connected).toBe(true);
    expect(state.snapshot?.revision).toBe(7);
    expect(audioStatus(received.routes[0], true)).toBe("On");
    expect(normalizeApiError(error)).toEqual(contract.error);
    expect(settings.settings.microphone.echoCancellation).toBe(true);
    expect(contract.pipelineStates.map((state) => audioStatus({ ...received.routes[0]!, state }, true)))
      .toEqual(["Off", "Starting…", "On", "Changing audio device…", "Reconnecting audio…", "Needs attention", "Stopping…"]);
  });

  it("delivers every Rust event through its real subscription topic", async () => {
    const observed: EngineEvent[] = [];
    const unsubscribe = vi.fn();
    mocks.listen.mockImplementation(async (topic: string, callback: (message: { payload: EngineEvent }) => void) => {
      const fixture = contract.events.find((event) => event.topic === topic)!;
      callback({ payload: JSON.parse(JSON.stringify(fixture.payload)) });
      return unsubscribe;
    });
    const stop = await subscribeToEngineEvents((event) => observed.push(event));
    expect(observed).toEqual(events);
    stop();
    expect(unsubscribe).toHaveBeenCalledTimes(4);
  });

  it("renders native update statuses, progress and signature failures", async () => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host);
    try {
      for (const fixture of updates) {
        const status: UpdateStatus = JSON.parse(JSON.stringify(fixture));
        await act(async () => root.render(<UpdateNotice helpOpen audioBusy updates={{ status, installing: status.phase === "installing", error: null, install: vi.fn(), check: vi.fn() }} />));
        expect(host.querySelector('[role="status"]')?.textContent).not.toBe("");
        if (status.phase === "downloading") expect(host.textContent).toContain("50%");
        if (status.phase === "error") expect(host.querySelector('[role="alert"]')?.textContent).toBe(status.error);
        if (status.phase === "ready") expect(host.querySelector("button")?.disabled).toBe(true);
      }
    } finally { await act(async () => root.unmount()); host.remove(); }
  });
});
