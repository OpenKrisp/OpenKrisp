import type { EngineEvent, EngineSnapshot } from "./api";

interface ConnectionOptions {
  getSnapshot: () => Promise<EngineSnapshot>;
  subscribe: (onEvent: (event: EngineEvent) => void) => Promise<() => void>;
  onSnapshot: (snapshot: EngineSnapshot) => void;
  onError: (error: unknown) => void;
  focusTarget?: EventTarget;
  intervalMs?: number;
}

// Events invalidate an authoritative snapshot; they never patch a partial view of a new route.
// One in-flight request plus a dirty bit bounds work even if notifications arrive in bursts.
export function connectEngine(options: ConnectionOptions): () => void {
  let closed = false;
  let refreshing = false;
  let dirty = false;
  let revision = -1;
  let subscribing = false;
  let unsubscribe: (() => void) | undefined;

  const refresh = async () => {
    dirty = true;
    if (refreshing || closed) return;
    refreshing = true;
    try {
      while (dirty && !closed) {
        dirty = false;
        const snapshot = await options.getSnapshot();
        if (!closed && snapshot.revision >= revision) {
          revision = snapshot.revision;
          options.onSnapshot(snapshot);
        }
      }
    } catch (error) {
      if (!closed) options.onError(error);
    } finally {
      refreshing = false;
    }
  };

  const subscribe = async () => {
    if (closed || subscribing || unsubscribe) return;
    subscribing = true;
    try {
      const remove = await options.subscribe((event) => {
        if (!closed && event.revision > revision) void refresh();
      });
      if (closed) remove();
      else {
        unsubscribe = remove;
        // Reconcile events missed while installing the listeners.
        void refresh();
      }
    } catch (error) {
      if (!closed) options.onError(error);
    } finally {
      subscribing = false;
    }
  };

  const reconcile = () => {
    void subscribe();
    void refresh();
  };
  // Periodic reconciliation repairs dropped events and retries an initial bridge failure.
  const interval = setInterval(reconcile, options.intervalMs ?? 1_000);
  options.focusTarget?.addEventListener("focus", reconcile);
  reconcile();
  return () => {
    closed = true;
    clearInterval(interval);
    options.focusTarget?.removeEventListener("focus", reconcile);
    unsubscribe?.();
  };
}
