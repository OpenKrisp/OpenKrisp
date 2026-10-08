import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { monitorUpdates, UPDATE_INTERVAL_MS, type UpdateStatus } from "./updates";

const status = (phase: UpdateStatus["phase"], revision = 0): UpdateStatus => ({
  revision, phase, version: phase === "ready" ? "1.1.0" : null, downloadedBytes: 0, totalBytes: null, error: null,
});
const monitors: ReturnType<typeof monitorUpdates>[] = [];
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(0); });
afterEach(() => { monitors.splice(0).forEach((monitor) => monitor.stop()); vi.useRealTimers(); });

function setup(initial = status("idle")) {
  const api = { status: vi.fn().mockResolvedValue(initial), prepare: vi.fn().mockResolvedValue(status("idle", 1)) };
  const onStatus = vi.fn(); const focus = new EventTarget();
  const monitor = monitorUpdates(api, onStatus, focus); monitors.push(monitor);
  return { api, onStatus, focus, monitor };
}

describe("automatic update checks", () => {
  it("checks on startup and every six hours and coalesces focus events", async () => {
    const { api, focus } = setup();
    await vi.advanceTimersByTimeAsync(0);
    expect(api.prepare).toHaveBeenCalledTimes(1);
    focus.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(UPDATE_INTERVAL_MS);
    expect(api.prepare).toHaveBeenCalledTimes(2);
    focus.dispatchEvent(new Event("focus"));
    expect(api.prepare).toHaveBeenCalledTimes(2);
  });

  it("leaves disabled builds and downloaded updates alone", async () => {
    for (const phase of ["disabled", "ready", "installing"] as const) {
      const { api } = setup(status(phase));
      await vi.advanceTimersByTimeAsync(0);
      expect(api.prepare).not.toHaveBeenCalled();
    }
  });

  it("polls download progress, rejects stale responses, and avoids overlapping downloads", async () => {
    const { api, onStatus, monitor } = setup();
    let finish!: (value: UpdateStatus) => void;
    api.prepare.mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    await vi.advanceTimersByTimeAsync(0);
    await monitor.check();
    expect(api.prepare).toHaveBeenCalledTimes(1);
    api.status.mockResolvedValue(status("downloading", 4));
    await vi.advanceTimersByTimeAsync(1000);
    expect(onStatus).toHaveBeenLastCalledWith(status("downloading", 4));
    api.status.mockResolvedValue(status("checking", 2));
    await vi.advanceTimersByTimeAsync(1000);
    expect(onStatus).toHaveBeenLastCalledWith(status("downloading", 4));
    finish(status("ready", 5));
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(UPDATE_INTERVAL_MS);
    expect(api.prepare).toHaveBeenCalledTimes(1);
    expect(onStatus).toHaveBeenLastCalledWith(status("ready", 5));
  });

  it("retries failed checks automatically and supports an immediate manual retry", async () => {
    const { api, monitor } = setup();
    api.prepare.mockResolvedValue({ ...status("error", 1), error: "Offline" });
    await vi.advanceTimersByTimeAsync(0);
    await monitor.check();
    expect(api.prepare).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(UPDATE_INTERVAL_MS);
    expect(api.prepare).toHaveBeenCalledTimes(3);
  });

  it("removes timers and ignores an in-flight response after cleanup", async () => {
    const { api, onStatus, focus, monitor } = setup();
    let finish!: (value: UpdateStatus) => void;
    api.prepare.mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    await vi.advanceTimersByTimeAsync(0);
    monitor.stop(); onStatus.mockClear();
    finish(status("ready", 2));
    focus.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(UPDATE_INTERVAL_MS);
    expect(onStatus).not.toHaveBeenCalled();
    expect(api.prepare).toHaveBeenCalledTimes(1);
  });
});
