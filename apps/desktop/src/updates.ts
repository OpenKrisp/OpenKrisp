import { invoke } from "@tauri-apps/api/core";

export interface UpdateStatus {
  revision: number;
  phase: "disabled" | "idle" | "checking" | "downloading" | "ready" | "installing" | "error";
  version: string | null;
  downloadedBytes: number;
  totalBytes: number | null;
  error: string | null;
}

export const updateApi = {
  status: () => invoke<UpdateStatus>("get_update_status"),
  prepare: () => invoke<UpdateStatus>("prepare_update"),
  install: () => invoke<void>("install_update"),
};

export const UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000;

export function updateError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) return String(error.message);
  return String(error);
}

/** Native state survives window hiding and UI reloads; overlapping checks are coalesced. */
export function monitorUpdates(api: Pick<typeof updateApi, "status" | "prepare">, onStatus: (status: UpdateStatus) => void, focusTarget: EventTarget = window) {
  let disposed = false;
  let checking = false;
  let polling = false;
  let lastCheck = -Infinity;
  let current: UpdateStatus | null = null;

  function apply(next: UpdateStatus) {
    if (disposed || (current && next.revision < current.revision)) return;
    current = next;
    onStatus(next);
  }

  async function refresh() {
    if (disposed || polling) return;
    polling = true;
    try { apply(await api.status()); }
    catch { /* A transient IPC failure does not erase a verified native update. */ }
    finally { polling = false; }
  }

  async function check() {
    if (disposed || checking || current?.phase === "disabled" || current?.phase === "ready" || current?.phase === "installing") return;
    checking = true;
    lastCheck = Date.now();
    try { apply(await api.prepare()); }
    catch (error) {
      if (!disposed) onStatus({ revision: current?.revision ?? 0, phase: "error", version: null,
        downloadedBytes: 0, totalBytes: null, error: updateError(error) });
    } finally { checking = false; }
  }

  function due() { if (Date.now() - lastCheck >= UPDATE_INTERVAL_MS) void check(); }
  const interval = setInterval(due, UPDATE_INTERVAL_MS);
  const progress = setInterval(() => {
    if (checking || current?.phase === "checking" || current?.phase === "downloading" || current?.phase === "installing") void refresh();
  }, 1000);
  focusTarget.addEventListener("focus", due);
  void refresh().then(check);
  return {
    check,
    refresh,
    stop() { disposed = true; clearInterval(interval); clearInterval(progress); focusTarget.removeEventListener("focus", due); },
  };
}
