// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { UpdateNotice } from "./UpdateNotice";
import type { UpdateStatus } from "./updates";

it("keeps installing unavailable while audio is busy and preserves retry after an installation error", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); const install = vi.fn();
  const status: UpdateStatus = { phase: "ready", revision: 3, version: "1.1.0", downloadedBytes: 100,
    totalBytes: 100, error: null };
  const updates = { status, installing: false, error: null, install, check: vi.fn() };
  try {
    await act(async () => root.render(<UpdateNotice updates={updates} helpOpen={false} audioBusy />));
    expect(host.textContent).toContain("Turn off both audio routes");
    expect(host.querySelector("button")?.disabled).toBe(true);
    await act(async () => root.render(<UpdateNotice updates={{ ...updates, error: "Installation cancelled" }} helpOpen={false} audioBusy={false} />));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("Installation cancelled");
    await act(async () => host.querySelector("button")!.click());
    expect(install).toHaveBeenCalledTimes(1);
    await act(async () => root.render(<UpdateNotice updates={{ ...updates, installing: true }} helpOpen={false} audioBusy={false} />));
    expect(host.textContent).toContain("will restart");
    expect(host.querySelector("button")?.disabled).toBe(true);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("installs automatically after 30 idle seconds, cancels for audio, and allows postponing", async () => {
  vi.useFakeTimers();
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); const install = vi.fn();
  const status: UpdateStatus = { phase: "ready", revision: 3, version: "1.1.0", downloadedBytes: 100,
    totalBytes: 100, error: null };
  const updates = { status, installing: false, error: null, install, check: vi.fn() };
  try {
    await act(async () => root.render(<UpdateNotice updates={updates} helpOpen={false} audioBusy={false} />));
    await act(async () => vi.advanceTimersByTimeAsync(20_000));
    expect(install).not.toHaveBeenCalled();
    await act(async () => root.render(<UpdateNotice updates={updates} helpOpen={false} audioBusy />));
    await act(async () => vi.advanceTimersByTimeAsync(30_000));
    expect(install).not.toHaveBeenCalled();
    await act(async () => root.render(<UpdateNotice updates={updates} helpOpen={false} audioBusy={false} />));
    await act(async () => vi.advanceTimersByTimeAsync(30_000));
    expect(install).toHaveBeenCalledTimes(1);
    await act(async () => [...host.querySelectorAll("button")].find((button) => button.textContent === "Later")!.click());
    await act(async () => root.render(<UpdateNotice updates={updates} helpOpen={false} audioBusy />));
    await act(async () => root.render(<UpdateNotice updates={updates} helpOpen={false} audioBusy={false} />));
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(install).toHaveBeenCalledTimes(1);
    expect(host.textContent).not.toContain("seconds");
  } finally { await act(async () => root.unmount()); host.remove(); vi.useRealTimers(); }
});
