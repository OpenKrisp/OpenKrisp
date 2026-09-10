import { useRef, useState } from "react";
import type { EngineSnapshot, RouteSnapshot, StreamDiagnostics } from "./api";
import { diagnosticReport, formatDescription, routeLabel } from "./diagnostics";

function Facts({ entries }: { entries: [string, string | number][] }) {
  return <dl className="facts diagnostics-facts">{entries.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>;
}
function StreamFacts({ label, stream }: { label: string; stream: StreamDiagnostics }) {
  return <><h4>{label} stream</h4><Facts entries={[
    ["Callbacks", stream.callbacks], ["Frames", stream.frames], ["Issues", stream.issues],
    ["Terminal issues", stream.terminalIssues], ["Native xruns", stream.xruns], ["Callback violations", stream.callbackViolations],
    ["Last issue", stream.lastIssue ?? "None"], ["Last terminal issue", stream.lastTerminalIssue ?? "None"],
  ]} /></>;
}
function RouteDiagnostics({ route }: { route: RouteSnapshot }) {
  const m = route.metrics;
  return <section className="route-diagnostics" aria-label={`${routeLabel(route)} diagnostics`}>
    <h3>{routeLabel(route)} · {route.state}</h3>
    <p>{route.inputDevice?.name ?? ("applicationStream" in route.config.source ? "Native application source" : "Input not available")} → {route.outputDevice?.name ?? "Output not available"}</p>
    <p>Input: {formatDescription(route.inputFormat)}</p><p>Output: {formatDescription(route.outputFormat)}</p>
    <p>Model: {route.model?.id ?? "Not loaded"}</p>
    <Facts entries={[
      ["Model sample rate", route.model ? `${route.model.sampleRateHz} Hz` : "—"],
      ["Model channels", route.model?.channels ?? "—"],
      ["Model frame", route.model ? `${route.model.frameSamplesPerChannel} samples/channel` : "—"],
      ["Model algorithmic delay", route.model ? `${route.model.algorithmicLatencySamples} samples` : "—"],
      ["Route latency (estimated)", `${m.estimatedLatencyMs.toFixed(2)} ms`], ["Target latency", `${route.config.targetLatencyMs} ms`],
      ["Uptime", `${route.uptimeSeconds.toFixed(1)} s`], ["Requested mode", route.config.processingMode === "speech_denoise" ? "Speech denoise" : "Bypass"],
      ["User bypass", route.bypassed ? "On" : "Off"], ["Denoise enabled", route.config.denoise.enabled ? "Yes" : "No"],
      ["Degraded bypass", m.degraded ? "Active" : "Inactive"], ["Overload events", m.overloadEvents], ["Skipped inferences", m.skippedInferences],
      ["Inference last", `${(m.inferenceLastUs / 1000).toFixed(3)} ms`], ["Inference p95", `${(m.inferenceP95Us / 1000).toFixed(3)} ms`],
      ["Inference p99", `${(m.inferenceP99Us / 1000).toFixed(3)} ms`], ["Missed deadlines", m.missedDeadlines],
      ["Input queue", `${m.inputRingFillFrames} frames`], ["Input high water", `${m.inputRingHighWaterFrames} frames`],
      ["Output queue", `${m.outputRingFillFrames} frames`], ["Output low water", `${m.outputRingLowWaterFrames} frames`],
      ["Input overruns", m.inputOverruns], ["Output underruns", m.outputUnderruns], ["Discontinuities", m.discontinuities],
      ["Model errors", route.modelErrors], ["Model resets", m.modelResets], ["Last reset", `${(m.resetLastUs / 1000).toFixed(3)} ms`],
      ["Total reset time", `${(m.resetTotalUs / 1000).toFixed(3)} ms`], ["Drift correction", `${m.driftCorrectionPpm.toFixed(0)} ppm`],
      ["Recovery attempts", route.recoveryAttempts], ["Successful recoveries", route.successfulRecoveries],
      ["Input peak / RMS", `${m.inputPeak.toFixed(4)} / ${m.inputRms.toFixed(4)}`], ["Output peak / RMS", `${m.outputPeak.toFixed(4)} / ${m.outputRms.toFixed(4)}`],
    ]} />
    <StreamFacts label="Input" stream={route.inputStream} /><StreamFacts label="Output" stream={route.outputStream} />
    {route.fault ? <p role="alert">{route.fault.code}: {route.fault.message} {route.fault.suggestedAction}</p> : null}
  </section>;
}

export function DiagnosticPanel({ snapshot, connected }: { snapshot: EngineSnapshot; connected: boolean }) {
  const [report, setReport] = useState("");
  const [copyMessage, setCopyMessage] = useState("");
  const textArea = useRef<HTMLTextAreaElement>(null);
  async function copy() {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(report);
      setCopyMessage("Report copied.");
    } catch {
      textArea.current?.focus(); textArea.current?.select();
      setCopyMessage("Automatic copy is unavailable. The report is selected; use your usual Copy shortcut.");
    }
  }
  return <details className="panel" onToggle={(event) => { if (!event.currentTarget.open) setCopyMessage(""); }}>
    <summary>Diagnostics</summary>
    <p className="hint">{connected ? "Connected to the engine." : "Disconnected: showing the last received snapshot."} Route latency is an estimate, not a measured end-to-end or acoustic delay.</p>
    {snapshot.routes.length === 0 ? <p>No routes have been started.</p> : snapshot.routes.map((route) => <RouteDiagnostics key={route.config.id} route={route} />)}
    <div className="report-controls">
      <h3>Share diagnostics</h3>
      <p>The report includes formats, settings and counters. It excludes device names and identifiers, file paths, profiles, error descriptions and audio.</p>
      <div className="actions"><button onClick={() => { setReport(diagnosticReport(snapshot, connected)); setCopyMessage(""); }}>{report ? "Refresh report" : "Prepare report"}</button>
        {report ? <button onClick={() => void copy()}>Copy report</button> : null}</div>
      {report ? <label>Diagnostic report<textarea ref={textArea} readOnly value={report} rows={12} spellCheck={false} /></label> : null}
      {copyMessage ? <p role="status">{copyMessage}</p> : null}
    </div>
  </details>;
}
