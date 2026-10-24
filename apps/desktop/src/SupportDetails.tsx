import { useI18n } from "./i18n";
import { useEffect, useRef, useState } from "react";
import type { EngineSnapshot } from "./api";
import { diagnosticReport } from "./diagnostics";

export function SupportDetails({ snapshot, connected }: { snapshot: EngineSnapshot; connected: boolean }) {
  const { t } = useI18n();
  const [report, setReport] = useState("");
  const [message, setMessage] = useState<"support.copied" | "support.manual" | null>(null);
  const [copying, setCopying] = useState(false);
  const textArea = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { if (report) { textArea.current?.focus(); textArea.current?.select(); } }, [report]);

  async function copy() {
    const details = diagnosticReport(snapshot, connected);
    setCopying(true);
    try {
      if (!navigator.clipboard?.writeText) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(details);
      setReport(""); setMessage("support.copied");
    } catch {
      setReport(details);
      textArea.current?.focus(); textArea.current?.select();
      setMessage("support.manual");
    } finally { setCopying(false); }
  }

  return <section className="help-action support-details" aria-labelledby="support-title">
    <div className="help-action-description"><h3 id="support-title">{t("support.title")}</h3>
      <p>{t("support.description")}</p>
      <p>{t("support.privacy")}</p></div>
    <button className="secondary" disabled={copying} onClick={() => void copy()}>{t("support.copy")}</button>
    {report ? <label className="help-action-feedback">{t("support.details")}<textarea ref={textArea} readOnly value={report} rows={6} spellCheck={false} /></label> : null}
    {message ? <p className="help-action-feedback" role="status">{t(message)}</p> : null}
  </section>;
}
