import { afterEach, describe, expect, it, vi } from "vitest";
import type { EngineEvent, EngineSnapshot } from "./api";
import { connectEngine } from "./connection";
import { route } from "./testFixtures";

const snapshot = (revision: number): EngineSnapshot => ({
  revision,
  shuttingDown: false,
  inventory: { host: "fake", devices: [] },
  routes: [],
  lastFault: null,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("engine connection reconciliation", () => {
  afterEach(() => vi.useRealTimers());

  it("coalesces bursts and reconciles a route created after the initial empty snapshot", async () => {
    vi.useFakeTimers();
    const pending = deferred<EngineSnapshot>();
    let onEvent!: (event: EngineEvent) => void;
    const getSnapshot = vi.fn<() => Promise<EngineSnapshot>>().mockResolvedValue(snapshot(1));
    const onSnapshot = vi.fn<(value: EngineSnapshot) => void>();
    const unsubscribe = vi.fn<() => void>();
    const disconnect = connectEngine({
      getSnapshot,
      onSnapshot,
      onError: vi.fn<() => void>(),
      subscribe: async (callback) => {
        onEvent = callback;
        return unsubscribe;
      },
    });
    await vi.advanceTimersByTimeAsync(0);
    getSnapshot.mockClear();
    const running = { ...snapshot(3), routes: [route] };
    getSnapshot.mockReturnValueOnce(pending.promise).mockResolvedValue(running);
    onEvent({ revision: 2, type: "state_changed", routeId: route.config.id, state: "starting" });
    for (let i = 0; i < 20; i++) onEvent({ revision: 3, type: "state_changed", routeId: "new", state: "running" });
    expect(getSnapshot).toHaveBeenCalledTimes(1);
    pending.resolve(snapshot(2));
    await vi.advanceTimersByTimeAsync(0);
    expect(getSnapshot).toHaveBeenCalledTimes(2);
    expect(onSnapshot).toHaveBeenLastCalledWith(running);
    // An old or duplicate event cannot regress state or trigger more requests.
    onEvent({ revision: 2, type: "state_changed", routeId: "new", state: "failed" });
    expect(getSnapshot).toHaveBeenCalledTimes(2);
    disconnect();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });

  it("repairs dropped events on a timer and on focus", async () => {
    vi.useFakeTimers();
    const focusTarget = new EventTarget();
    const getSnapshot = vi.fn<() => Promise<EngineSnapshot>>().mockResolvedValue(snapshot(1));
    const onSnapshot = vi.fn<(value: EngineSnapshot) => void>();
    const disconnect = connectEngine({
      getSnapshot,
      onSnapshot,
      onError: vi.fn<() => void>(),
      focusTarget,
      subscribe: async () => vi.fn<() => void>(),
    });
    await vi.advanceTimersByTimeAsync(0);
    const stopped = { ...snapshot(4), shuttingDown: true };
    getSnapshot.mockResolvedValue(stopped);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(onSnapshot).toHaveBeenLastCalledWith(stopped);
    getSnapshot.mockResolvedValue(snapshot(5));
    focusTarget.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(0);
    expect(onSnapshot).toHaveBeenLastCalledWith(snapshot(5));
    disconnect();
    const calls = getSnapshot.mock.calls.length;
    focusTarget.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(2_000);
    expect(getSnapshot).toHaveBeenCalledTimes(calls);
  });

  it("retries failed startup subscriptions and snapshots without remounting", async () => {
    vi.useFakeTimers();
    const getSnapshot = vi.fn<() => Promise<EngineSnapshot>>()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue(snapshot(2));
    const subscribe = vi.fn<() => Promise<() => void>>()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue(vi.fn<() => void>());
    const onSnapshot = vi.fn<(value: EngineSnapshot) => void>();
    const onError = vi.fn<() => void>();
    const disconnect = connectEngine({ getSnapshot, subscribe, onSnapshot, onError });
    await vi.advanceTimersByTimeAsync(0);
    expect(onError).toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(subscribe).toHaveBeenCalledTimes(2);
    expect(onSnapshot).toHaveBeenLastCalledWith(snapshot(2));
    disconnect();
  });

  it("cleans up a late subscription and ignores an in-flight snapshot after unmount", async () => {
    vi.useFakeTimers();
    const subscription = deferred<() => void>();
    const pending = deferred<EngineSnapshot>();
    const onSnapshot = vi.fn<(value: EngineSnapshot) => void>();
    const remove = vi.fn<() => void>();
    const disconnect = connectEngine({
      getSnapshot: () => pending.promise,
      subscribe: () => subscription.promise,
      onSnapshot,
      onError: vi.fn<() => void>(),
    });
    disconnect();
    subscription.resolve(remove);
    pending.resolve(snapshot(7));
    await vi.advanceTimersByTimeAsync(0);
    expect(remove).toHaveBeenCalledOnce();
    expect(onSnapshot).not.toHaveBeenCalled();
  });
});
