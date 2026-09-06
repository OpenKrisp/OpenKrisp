import { describe, expect, it } from "vitest";

import type {
  EngineSnapshot,
  PipelineMetrics,
  RouteSnapshot,
  StreamDiagnostics,
} from "./api";
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

const streamDiagnostics: StreamDiagnostics = {
  callbacks: 0,
  frames: 0,
  issues: 0,
  terminalIssues: 0,
  xruns: 0,
  callbackViolations: 0,
  lastIssue: null,
  lastTerminalIssue: null,
};

const route: RouteSnapshot = {
  config: {
    id: "desktop-monitor",
    source: { physicalInput: "fake:input" },
    destination: { physicalOutput: "fake:output" },
    processingMode: "speech_denoise",
    targetLatencyMs: 40,
    denoise: {
      enabled: true,
      attenuationLimitDb: 24,
      postFilterEnabled: true,
    },
  },
  state: "running",
  bypassed: false,
  inputDevice: null,
  outputDevice: null,
  inputFormat: null,
  outputFormat: null,
  model: null,
  metrics,
  inputStream: streamDiagnostics,
  outputStream: streamDiagnostics,
  modelErrors: 0,
  recoveryAttempts: 0,
  successfulRecoveries: 0,
  uptimeSeconds: 1,
  fault: null,
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

  it("repairs event-derived state from a newer full snapshot", () => {
    const withRoute = { ...snapshot, routes: [route] };
    let state = reduceDesktopState(initialDesktopState, {
      type: "reconcile",
      snapshot: withRoute,
    });
    state = reduceDesktopState(state, {
      type: "event",
      event: { type: "state_changed", routeId: route.config.id, state: "failed" },
    });
    state = reduceDesktopState(state, {
      type: "event",
      event: {
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
