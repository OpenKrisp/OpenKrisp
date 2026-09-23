import type { RoutingSnapshot, PlatformEndpoint } from "./api";
import { AudioIcon } from "./AudioControls";

function needsSetup(endpoint: PlatformEndpoint) {
  return endpoint.availability !== "ready" && !(endpoint.availability === "missing" && endpoint.canCreate);
}

function ConnectionGuide({ kind, supported, endpoints }: {
  kind: "microphone" | "speaker"; supported: boolean; endpoints: PlatformEndpoint[];
}) {
  const microphone = kind === "microphone";
  const titleId = microphone ? "setup-title" : "speaker-help-title";
  const endpoint = endpoints.length === 1 ? endpoints[0] : undefined;

  return <section className="connection-guide" aria-labelledby={titleId}>
    <div className="help-section-heading"><AudioIcon kind={microphone ? "microphone" : "headphones"} />
      <h2 id={titleId}>{microphone ? "Connect your microphone" : "Connect your speakers"}</h2></div>
    {!supported ? <p>This installation can’t provide a call {kind}. Install OpenKrisp with its audio devices for macOS or Linux.</p>
      : !endpoint ? <p>{endpoints.length === 0
        ? `OpenKrisp’s ${kind} wasn’t found. Refresh devices or run the OpenKrisp installer again.`
        : `OpenKrisp found more than one call ${kind}. Refresh devices; if this continues, copy support details below.`}</p>
      : needsSetup(endpoint) ? <p className="notice"><strong>{endpoint.name}</strong>{endpoint.availability === "missing" ? "Installation needed." : "This audio device needs repair."} Run the OpenKrisp installer, restart if prompted, then refresh devices.</p>
      : <>
          <ol className="connection-steps" role="list">
            <li><span>{microphone ? "Choose your microphone on the home screen." : "Under Your speakers, choose where to listen."}</span></li>
            <li><span>Turn on OpenKrisp.</span></li>
            <li><span>In your call app, choose <strong>{endpoint.name}</strong> as the {kind}.</span></li>
          </ol>
          <p className="connection-note"><AudioIcon kind="headphones" /><span>{microphone
            ? "Keep your usual headphones or speakers selected if you only use microphone noise reduction."
            : "Before turning off OpenKrisp, switch your call app back to your usual headphones or speakers."}</span></p>
        </>}
  </section>;
}

export function EndpointSetup({ routing }: { routing: RoutingSnapshot }) {
  return <div className="connection-guides">
    <ConnectionGuide kind="microphone" supported={routing.capabilities.virtualMicrophone} endpoints={routing.endpoints} />
    <ConnectionGuide kind="speaker" supported={routing.capabilities.systemLoopback} endpoints={routing.loopbackEndpoints ?? []} />
  </div>;
}
