import type { AudioFormat, EngineSnapshot, PipelineMetrics, RouteSnapshot, StreamDiagnostics } from "./api";

const errorCodes = ["invalid_identifier", "invalid_configuration", "invalid_state", "command_queue_full", "device_not_found",
  "device_disconnected", "unsupported_format", "audio_io", "dsp", "denoise", "model_asset_missing", "model_hash_mismatch",
  "model_load_failed", "invalid_model_frame", "non_finite_audio", "stream_error", "routing", "worker_terminated", "shutdown_in_progress", "internal"];
const issueCodes = ["DeviceBusy", "DeviceChanged", "DeviceNotAvailable", "HostUnavailable", "InvalidInput", "PermissionDenied",
  "RealtimeDenied", "ResourceExhausted", "StreamInvalidated", "UnsupportedConfig", "UnsupportedOperation", "Xrun", "Backend", "Other"];
const metricKeys = ["inputPeak", "inputRms", "outputPeak", "outputRms", "inferenceLastUs", "inferenceP95Us", "inferenceP99Us",
  "missedDeadlines", "inputRingFillFrames", "outputRingFillFrames", "inputRingHighWaterFrames", "outputRingLowWaterFrames",
  "inputOverruns", "outputUnderruns", "discontinuities", "modelResets", "resetLastUs", "resetTotalUs", "driftCorrectionPpm",
  "overloadEvents", "skippedInferences", "estimatedLatencyMs"] as const satisfies readonly (keyof PipelineMetrics)[];
const category = (value: string | null, allowed: readonly string[]) => value === null ? null : allowed.includes(value) ? value : "other";
const finite = (value: number | null) => typeof value === "number" && Number.isFinite(value) ? value : null;

function format(format: AudioFormat | null) {
  return format ? {
    sampleRateHz: finite(format.sampleRateHz), channels: finite(format.channels),
    sampleFormat: category(format.sampleFormat, ["i8", "i16", "i24", "i32", "i64", "u8", "u16", "u24", "u32", "u64", "f32", "f64"]),
    bufferSizeFrames: finite(format.bufferSizeFrames),
  } : null;
}

function stream(stream: StreamDiagnostics) {
  return {
    callbacks: finite(stream.callbacks), frames: finite(stream.frames), issues: finite(stream.issues),
    terminalIssues: finite(stream.terminalIssues), xruns: finite(stream.xruns), callbackViolations: finite(stream.callbackViolations),
    lastIssue: category(stream.lastIssue, issueCodes), lastTerminalIssue: category(stream.lastTerminalIssue, issueCodes),
  };
}

export function routeLabel(route: RouteSnapshot): string {
  return "physicalInput" in route.config.source ? "Microphone" : "Incoming audio";
}

// Build an allowlisted report instead of redacting a serialized snapshot. New DTO fields stay
// private by default. Neither identifiers nor arbitrary backend strings enter this object.
export function diagnosticReport(snapshot: EngineSnapshot, connected: boolean): string {
  return JSON.stringify({
    reportVersion: 1, application: "OpenKrisp", connected,
    host: category(snapshot.inventory.host, ["coreaudio", "pipewire", "alsa", "jack", "pulse", "pulseaudio"]),
    shuttingDown: snapshot.shuttingDown === true,
    latencyDescription: "Route latency is estimated, not measured end-to-end or acoustic latency.",
    lastFaultCode: category(snapshot.lastFault?.code ?? null, errorCodes),
    routes: snapshot.routes.slice(0, 2).map((route) => ({
      role: routeLabel(route),
      sourceKind: "physicalInput" in route.config.source ? "physical_input" : "systemLoopback" in route.config.source ? "system_loopback" : "application_stream",
      destinationKind: "physicalOutput" in route.config.destination ? "physical_output" : "virtual_microphone",
      state: category(route.state, ["stopped", "starting", "running", "reconfiguring", "recovering", "failed", "stopping"]),
      mode: category(route.config.processingMode, ["speech_denoise", "aligned_bypass"]),
      bypassed: route.bypassed === true,
      targetLatencyMs: finite(route.config.targetLatencyMs), uptimeSeconds: finite(route.uptimeSeconds),
      denoise: { enabled: route.config.denoise.enabled === true, attenuationLimitDb: finite(route.config.denoise.attenuationLimitDb), postFilterEnabled: route.config.denoise.postFilterEnabled === true },
      inputFormat: format(route.inputFormat), outputFormat: format(route.outputFormat),
      model: route.model ? {
        id: category(route.model.id, ["deepfilternet3-low-latency-v0.5.6"]),
        sampleRateHz: finite(route.model.sampleRateHz), channels: finite(route.model.channels),
        frameSamplesPerChannel: finite(route.model.frameSamplesPerChannel), algorithmicLatencySamples: finite(route.model.algorithmicLatencySamples),
      } : null,
      metrics: { ...Object.fromEntries(metricKeys.map((key) => [key, finite(route.metrics[key])])), degraded: route.metrics.degraded === true },
      inputStream: stream(route.inputStream), outputStream: stream(route.outputStream),
      modelErrors: finite(route.modelErrors), recoveryAttempts: finite(route.recoveryAttempts), successfulRecoveries: finite(route.successfulRecoveries),
      faultCode: category(route.fault?.code ?? null, errorCodes),
    })),
  }, null, 2);
}

export function formatDescription(value: AudioFormat | null): string {
  return value ? `${value.sampleRateHz} Hz · ${value.channels} ch · ${value.sampleFormat} · ${value.bufferSizeFrames ?? "default"} frames` : "Not negotiated";
}
