import type { EngineSnapshot, RouteSnapshot } from "./api";

function statusText(route: RouteSnapshot | undefined, enabled: boolean, connected: boolean, snapshot: EngineSnapshot | null): string {
  if (!connected) return "Echo cancellation status unavailable while disconnected.";
  if (!route || route.state === "stopped" || route.state === "failed") {
    return enabled ? "Echo cancellation is on for the next microphone start." : "Echo cancellation is off.";
  }
  if (!route.config.echoCancellation) return "Echo cancellation is off.";
  if (route.state !== "running") return "Echo cancellation is waiting for the microphone to run.";
  const incomingRunning = snapshot?.routes.some((candidate) =>
    !("physicalInput" in candidate.config.source) && candidate.state === "running");
  if (!incomingRunning || route.echoCancellation?.status === "waiting_reference") {
    return "Echo cancellation is waiting for incoming audio. Start incoming audio and send your call’s speaker output through it. Your microphone currently has no echo protection.";
  }
  switch (route.echoCancellation?.status) {
    case "running": return "Echo cancellation is active for audio playing through OpenKrisp.";
    case "aligning": return "Echo cancellation is aligning with playback. Your microphone currently has no echo protection.";
    case "overloaded": return "Echo cancellation is unavailable because processing is overloaded. Your microphone continues without echo protection; use headphones.";
    case "invalid_timing": return "Echo cancellation is waiting for stable playback timing. Your microphone continues without echo protection; use headphones.";
    default: return "Echo cancellation is unavailable. Your microphone continues without echo protection; use headphones.";
  }
}

export function EchoCancellation({ route, enabled, connected, snapshot }: {
  route: RouteSnapshot | undefined; enabled: boolean; connected: boolean; snapshot: EngineSnapshot | null;
}) {
  return <p role="status" className="hint" aria-label="Echo cancellation status">{statusText(route, enabled, connected, snapshot)}</p>;
}
