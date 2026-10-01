import { useEffect, useRef, useState } from "react";
import type { EngineSnapshot } from "./api";
import { diagnosticReport } from "./diagnostics";

export function SupportDetails({ snapshot, connected }: { snapshot: EngineSnapshot; connected: boolean }) {
  const [report, setReport] = useState("");
  const [message, setMessage] = useState("");
  const [copying, setCopying] = useState(false);
  const textArea = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { if (report) { textArea.current?.focus(); textArea.current?.select(); } }, [report]);

  async function copy() {
    const details = diagnosticReport(snapshot, connected);
    setCopying(true);
    try {
      if (!navigator.clipboard?.writeText) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(details);
      setReport(""); setMessage("Support details copied.");
    } catch {
      setReport(details);
      textArea.current?.focus(); textArea.current?.select();
      setMessage("Select and copy these details to share them with support.");
    } finally { setCopying(false); }
  }

  return <section className="help-action support-details" aria-labelledby="support-title">
    <div className="help-action-description"><h3 id="support-title">Still need help?</h3>
      <p>Copy details to share with support.</p>
      <p>No recordings or device names.</p></div>
    <button className="secondary" disabled={copying} onClick={() => void copy()}>Copy support details</button>
    {report ? <label className="help-action-feedback">Support details<textarea ref={textArea} readOnly value={report} rows={6} spellCheck={false} /></label> : null}
    {message ? <p className="help-action-feedback" role="status">{message}</p> : null}
  </section>;
}
