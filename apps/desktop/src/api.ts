import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type PipelineState =
  | "stopped"
  | "starting"
  | "running"
  | "reconfiguring"
  | "recovering"
  | "failed"
  | "stopping";

export type ProcessingMode = "speech_denoise" | "aligned_bypass";

export interface ApiError {
  code: string;
  message: string;
  details: string | null;
  suggestedAction: string | null;
}

export interface DenoiseSettings {
  enabled: boolean;
  attenuationLimitDb: number | null;
  postFilterEnabled: boolean;
}

export interface AudioFormat {
  sampleRateHz: number;
  channels: number;
  sampleFormat: string;
  bufferSizeFrames: number | null;
}

export interface AudioDevice {
  kind: "physical" | "virtual" | "aggregate" | "unknown";
  id: string;
  name: string;
  host: string;
  manufacturer: string | null;
  driver: string | null;
  supportsInput: boolean;
  supportsOutput: boolean;
  isDefaultInput: boolean;
  isDefaultOutput: boolean;
  defaultInputFormat: AudioFormat | null;
  defaultOutputFormat: AudioFormat | null;
}

export interface DeviceInventory {
  host: string;
  devices: AudioDevice[];
}

export interface PipelineMetrics {
  inputPeak: number;
  inputRms: number;
  outputPeak: number;
  outputRms: number;
  inferenceLastUs: number;
  inferenceP95Us: number;
  inferenceP99Us: number;
  missedDeadlines: number;
  inputRingFillFrames: number;
  outputRingFillFrames: number;
  inputRingHighWaterFrames: number;
  outputRingLowWaterFrames: number;
  inputOverruns: number;
  outputUnderruns: number;
  discontinuities: number;
  modelResets: number;
  resetLastUs: number;
  resetTotalUs: number;
  driftCorrectionPpm: number;
  degraded: boolean;
  overloadEvents: number;
  skippedInferences: number;
  estimatedLatencyMs: number;
}

export interface ModelInfo {
  id: string;
  sampleRateHz: number;
  channels: number;
  frameSamplesPerChannel: number;
  algorithmicLatencySamples: number;
}

export interface StreamDiagnostics {
  callbacks: number;
  frames: number;
  issues: number;
  terminalIssues: number;
  xruns: number;
  callbackViolations: number;
  lastIssue: string | null;
  lastTerminalIssue: string | null;
}

export interface RouteConfig {
  id: string;
  source: { physicalInput: string } | { systemLoopback: string } | { applicationStream: string };
  destination: { physicalOutput: string } | { virtualMicrophone: string };
  processingMode: ProcessingMode;
  targetLatencyMs: number;
  denoise: DenoiseSettings;
  echoCancellation: boolean;
}

export interface EchoCancellationSnapshot {
  status: "waiting_reference" | "aligning" | "running" | "invalid_timing" | "native_error" | "overloaded" | "stopped";
  processedFrames: number;
  fallbackFrames: number;
  resets: number;
  nativeErrors: number;
  maxProcessingUs: number;
  missedDeadlines: number;
}

export interface RouteSnapshot {
  config: RouteConfig;
  state: PipelineState;
  bypassed: boolean;
  inputDevice: AudioDevice | null;
  outputDevice: AudioDevice | null;
  inputFormat: AudioFormat | null;
  outputFormat: AudioFormat | null;
  model: ModelInfo | null;
  metrics: PipelineMetrics;
  echoCancellation: EchoCancellationSnapshot | null;
  inputStream: StreamDiagnostics;
  outputStream: StreamDiagnostics;
  modelErrors: number;
  recoveryAttempts: number;
  successfulRecoveries: number;
  uptimeSeconds: number;
  fault: ApiError | null;
}

export interface RoutingSnapshot {
  capabilities: {
    physicalInput: boolean;
    physicalOutput: boolean;
    systemLoopback: boolean;
    virtualMicrophone: boolean;
    virtualSpeaker: boolean;
    requiresExternalDriver: boolean;
    supportsRenderReference: boolean;
  };
  endpoints: PlatformEndpoint[];
  loopbackEndpoints?: PlatformEndpoint[];
}

export interface PlatformEndpoint {
  id: string;
  name: string;
  availability: "ready" | "missing" | "unsupported" | "invalid";
  canCreate: boolean;
  playbackDeviceId: string | null;
  captureDeviceId: string | null;
  ancestry: { recursivePaths: string[]; complete: boolean };
  instructions: string[];
  setupUrl: string;
}

export interface EngineSnapshot {
  revision: number;
  shuttingDown: boolean;
  inventory: DeviceInventory;
  routing: RoutingSnapshot;
  routes: RouteSnapshot[];
  lastFault: ApiError | null;
}

export interface AudioProfile {
  id: string;
  name: string;
  sourceKind: "physical_input" | "system_loopback";
  destinationKind: "physical_output" | "virtual_microphone";
  processingMode: ProcessingMode;
  sourceDevicePreference: string;
  destinationDevicePreference: string;
  targetLatencyMs: number;
  denoise: DenoiseSettings;
  echoCancellation: boolean;
}

export interface ProfileCollection {
  schemaVersion: number;
  selectedProfileId: string;
  profiles: AudioProfile[];
  loadWarning: ApiError | null;
}

export type EngineEvent = { revision: number } & (
  | { type: "state_changed"; routeId: string; state: PipelineState }
  | { type: "devices_changed" }
  | { type: "metrics"; routeId: string; metrics: PipelineMetrics }
  | { type: "fault"; routeId: string | null; error: ApiError; recoverable: boolean }
);

export interface StartRouteRequest {
  routeId: string;
  sourceDeviceId: string;
  sourceKind?: "physical_input" | "system_loopback";
  destinationDeviceId: string;
  destinationKind?: "physical_output" | "virtual_microphone";
  mode?: ProcessingMode;
  targetLatencyMs: number;
  denoise: DenoiseSettings;
  echoCancellation?: boolean;
}

const eventTopics = [
  "audio://state-changed",
  "audio://devices-changed",
  "audio://metrics",
  "audio://fault",
] as const;

export const api = {
  listAudioDevices: () => invoke<DeviceInventory>("list_audio_devices"),
  refreshAudioDevices: () => invoke<void>("refresh_audio_devices"),
  startRoute: (request: StartRouteRequest) =>
    invoke<{ routeId: string }>("start_route", { request }),
  stopRoute: (routeId: string) => invoke<void>("stop_route", { routeId }),
  updateRouteSettings: (routeId: string, denoise: DenoiseSettings) =>
    invoke<void>("update_route_settings", { request: { routeId, denoise } }),
  setRouteBypass: (routeId: string, bypassed: boolean) =>
    invoke<void>("set_route_bypass", { request: { routeId, bypassed } }),
  getEngineSnapshot: () => invoke<EngineSnapshot>("get_engine_snapshot"),
  listProfiles: () => invoke<ProfileCollection>("list_profiles"),
  saveProfile: (profile: AudioProfile) => invoke<void>("save_profile", { profile }),
  deleteProfile: (profileId: string) => invoke<void>("delete_profile", { profileId }),
  selectProfile: (profileId: string) => invoke<void>("select_profile", { profileId }),
};

export async function subscribeToEngineEvents(
  onEvent: (event: EngineEvent) => void,
): Promise<() => void> {
  const unlisteners: UnlistenFn[] = [];
  try {
    for (const topic of eventTopics) {
      unlisteners.push(await listen<EngineEvent>(topic, ({ payload }) => onEvent(payload)));
    }
  } catch (error) {
    for (const unlisten of unlisteners) {
      unlisten();
    }
    throw error;
  }
  return () => {
    for (const unlisten of unlisteners) {
      unlisten();
    }
  };
}
