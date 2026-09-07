import { describe, expect, it } from "vitest";
import { initialDesktopState, normalizeApiError, reduceDesktopState } from "./state";
import { metrics, snapshot, route } from "./testFixtures";

describe("desktop state reconciliation", () => {
  it("uses the full snapshot as the source of truth", () => {
    const state = reduceDesktopState(initialDesktopState, { type: "reconcile", snapshot });
    expect(state.connected).toBe(true);
    expect(state.snapshot?.revision).toBe(1);
  });

  it("does not let an older asynchronous snapshot overwrite newer state", () => {
    const newer = { ...snapshot, revision: 2 };
    const state = reduceDesktopState(initialDesktopState, { type: "reconcile", snapshot: newer });
    const reconciled = reduceDesktopState(state, { type: "reconcile", snapshot });
    expect(reconciled.snapshot?.revision).toBe(2);
  });

  it("ignores metrics for an absent route until the next snapshot", () => {
    const state = reduceDesktopState(
      reduceDesktopState(initialDesktopState, { type: "reconcile", snapshot }),
      { type: "event", event: { revision: 2, type: "metrics", routeId: "missing", metrics } },
    );
    expect(state.snapshot?.routes).toEqual([]);
  });

  it("keeps full snapshots authoritative over partial events", () => {
    const withRoute = { ...snapshot, routes: [route] };
    let state = reduceDesktopState(initialDesktopState, {
      type: "reconcile",
      snapshot: withRoute,
    });
    state = reduceDesktopState(state, {
      type: "event",
      event: { revision: 2, type: "state_changed", routeId: route.config.id, state: "failed" },
    });
    state = reduceDesktopState(state, {
      type: "event",
      event: {
        revision: 2,
        type: "metrics",
        routeId: route.config.id,
        metrics: { ...metrics, inputPeak: 0.95, degraded: true },
      },
    });

    const recovered = reduceDesktopState(state, {
      type: "reconcile",
      snapshot: { ...withRoute, revision: 2 },
    });

    expect(recovered.snapshot?.routes[0]?.state).toBe("running");
    expect(recovered.snapshot?.routes[0]?.metrics.inputPeak).toBe(0);
    expect(recovered.snapshot?.routes[0]?.metrics.degraded).toBe(false);
  });

  it("normalizes non-DTO bridge failures", () => {
    expect(normalizeApiError(new Error("not in Tauri")).code).toBe(
      "desktop_bridge_unavailable",
    );
  });
});
