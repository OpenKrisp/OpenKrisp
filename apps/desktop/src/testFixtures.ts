import type { EngineSnapshot, PipelineMetrics, RouteSnapshot, StreamDiagnostics } from "./api";

export const metrics: PipelineMetrics = {
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
  resetLastUs: 0,
  resetTotalUs: 0,
  driftCorrectionPpm: 0,
  degraded: false,
  overloadEvents: 0,
  skippedInferences: 0,
  estimatedLatencyMs: 0,
};

export const snapshot: EngineSnapshot = {
  revision: 1,
  shuttingDown: false,
  inventory: { host: "fake", devices: [] },
  routing: {
    capabilities: {
      physicalInput: true, physicalOutput: true, systemLoopback: false,
      virtualMicrophone: false, virtualSpeaker: false, requiresExternalDriver: false,
      supportsRenderReference: false,
    },
    endpoints: [],
  },
  routes: [],
  lastFault: null,
};

export const streamDiagnostics: StreamDiagnostics = {
  callbacks: 0,
  frames: 0,
  issues: 0,
  terminalIssues: 0,
  xruns: 0,
  callbackViolations: 0,
  lastIssue: null,
  lastTerminalIssue: null,
};

export const route: RouteSnapshot = {
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
