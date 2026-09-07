import type { ApiError, EngineEvent, EngineSnapshot } from "./api";

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
  // Partial/lossy events are handled as invalidations by the connection, never as state.
  return state;
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
