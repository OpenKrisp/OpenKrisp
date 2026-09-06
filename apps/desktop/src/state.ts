import type { ApiError, EngineEvent, EngineSnapshot, PipelineMetrics } from "./api";

export interface DesktopState {
  snapshot: EngineSnapshot | null;
  connected: boolean;
  fault: ApiError | null;
}

export const initialDesktopState: DesktopState = {
  snapshot: null,
  connected: false,
  fault: null,
};

export type DesktopAction =
  | { type: "reconcile"; snapshot: EngineSnapshot }
  | { type: "event"; event: EngineEvent }
  | { type: "connection_failed"; error: ApiError };

export function reduceDesktopState(state: DesktopState, action: DesktopAction): DesktopState {
  if (action.type === "reconcile") {
    if (state.snapshot !== null && action.snapshot.revision < state.snapshot.revision) {
      return state;
    }
    return {
      snapshot: action.snapshot,
      connected: true,
      fault: action.snapshot.lastFault,
    };
  }
  if (action.type === "connection_failed") {
    return { ...state, connected: false, fault: action.error };
  }
  if (state.snapshot === null) {
    return action.event.type === "fault" ? { ...state, fault: action.event.error } : state;
  }

  const snapshot = state.snapshot;
  switch (action.event.type) {
    case "state_changed": {
      const { routeId, state: routeState } = action.event;
      return {
        ...state,
        snapshot: {
          ...snapshot,
          routes: snapshot.routes.map((route) =>
            route.config.id === routeId ? { ...route, state: routeState } : route,
          ),
        },
      };
    }
    case "metrics":
      return updateMetrics(state, action.event.routeId, action.event.metrics);
    case "fault":
      return { ...state, fault: action.event.error };
    case "devices_changed":
      // Device events are an invalidation signal. The caller obtains a fresh full snapshot.
      return state;
  }
}

function updateMetrics(
  state: DesktopState,
  routeId: string,
  metrics: PipelineMetrics,
): DesktopState {
  const snapshot = state.snapshot;
  if (snapshot === null) {
    return state;
  }
  return {
    ...state,
    snapshot: {
      ...snapshot,
      routes: snapshot.routes.map((route) =>
        route.config.id === routeId ? { ...route, metrics } : route,
      ),
    },
  };
}

export function normalizeApiError(error: unknown): ApiError {
  if (typeof error === "object" && error !== null && "code" in error && "message" in error) {
    const candidate = error as Partial<ApiError>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return {
        code: candidate.code,
        message: candidate.message,
        details: candidate.details ?? null,
        suggestedAction: candidate.suggestedAction ?? null,
      };
    }
  }
  return {
    code: "desktop_bridge_unavailable",
    message: "The desktop control plane is unavailable.",
    details: error instanceof Error ? error.message : null,
    suggestedAction: "Restart OpenKrisp.",
  };
}
