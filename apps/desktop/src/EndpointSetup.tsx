import type { RoutingSnapshot } from "./api";

export function EndpointSetup({ routing }: { routing: RoutingSnapshot }) {
  if (!routing.capabilities.virtualMicrophone) {
    return <section className="panel" aria-labelledby="setup-title">
      <h2 id="setup-title">Virtual microphone</h2>
      <p>Virtual microphone routing is unavailable in this build. Microphone monitoring is available on this platform.</p>
    </section>;
  }
  return <section className="panel" aria-labelledby="setup-title">
    <h2 id="setup-title">Set up your virtual microphone</h2>
    {routing.endpoints.map((endpoint) => <div key={endpoint.id}>
      <p><strong>{endpoint.name}</strong> · {endpoint.availability === "ready" ? "Ready" : endpoint.availability === "missing" ? endpoint.canCreate ? "Created when you start" : "Installation needed" : endpoint.availability === "invalid" ? "Unsupported device configuration" : "Unavailable on this audio host"}</p>
      <ol>{endpoint.instructions.map((instruction) => <li key={instruction}>{instruction}</li>)}</ol>
      <p>Setup instructions: <code>{endpoint.setupUrl}</code></p>
    </div>)}
    <p>OpenKrisp sends the cleaned microphone to the receiving app. Use headphones for call playback. Stop the route to stop sending microphone audio.</p>
  </section>;
}
