import { describe, expect, it } from "vitest";

import type { EngineSnapshot, PipelineMetrics } from "./api";
import { initialDesktopState, normalizeApiError, reduceDesktopState } from "./state";

const metrics: PipelineMetrics = {
  inputPeak: 0,
  inputRms: 0,
  outputPeak: 0,
  outputRms: 0,
  inferenceLastUs: 0,
  inferenceP95Us: 0,
  inferenceP99Us: 0,
  missedDeadlines: 0,
  inputRingFillFrames: 0,
  outputRingFillFrames: 0,
  inputRingHighWaterFrames: 0,
  outputRingLowWaterFrames: 0,
  inputOverruns: 0,
  outputUnderruns: 0,
  discontinuities: 0,
  modelResets: 0,
  driftCorrectionPpm: 0,
  degraded: false,
  overloadEvents: 0,
  skippedInferences: 0,
  estimatedLatencyMs: 0,
};

const snapshot: EngineSnapshot = {
  revision: 1,
  shuttingDown: false,
  inventory: { host: "fake", devices: [] },
  routes: [],
  lastFault: null,
};

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
      { type: "event", event: { type: "metrics", routeId: "missing", metrics } },
    );
    expect(state.snapshot?.routes).toEqual([]);
  });

  it("normalizes non-DTO bridge failures", () => {
    expect(normalizeApiError(new Error("not in Tauri")).code).toBe(
      "desktop_bridge_unavailable",
    );
  });
});
