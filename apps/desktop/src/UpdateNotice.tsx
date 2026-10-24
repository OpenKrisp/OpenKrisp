import { ErrorDetails, useI18n } from "./i18n";
import { useEffect, useState } from "react";
import { pluralMessageId } from "./translations";
import type { useUpdates } from "./useUpdates";

export function UpdateNotice({ updates, helpOpen, audioBusy }: {
  updates: ReturnType<typeof useUpdates>; helpOpen: boolean; audioBusy: boolean;
}) {
  const { t, language } = useI18n();
  const status = updates.status;
  const [deferredVersion, setDeferredVersion] = useState<string | null>(null);
  const [seconds, setSeconds] = useState(30);
  const automatic = status?.phase === "ready" && !audioBusy && !updates.installing &&
    !updates.error && !status.error && deferredVersion !== status.version;
  const install = updates.install;
  useEffect(() => {
    if (!automatic) return;
    setSeconds(30);
    const deadline = Date.now() + 30_000;
    const countdown = setInterval(() => setSeconds(Math.max(0, Math.ceil((deadline - Date.now()) / 1000))), 1000);
    const timer = setTimeout(() => { void install(); }, 30_000);
    return () => { clearInterval(countdown); clearTimeout(timer); };
  }, [automatic, status?.version, install]);
  if (!status) return null;
  const ready = status.phase === "ready";
  if (!helpOpen && !ready && !updates.installing) return null;
  const progress = status.totalBytes ? Math.min(100, Math.floor(status.downloadedBytes / status.totalBytes * 100)) : null;
  const message = updates.installing ? t("updates.installing")
    : ready ? t("updates.ready", { version: status.version ?? "" })
    : status.phase === "checking" ? t("updates.checking")
    : status.phase === "downloading" ? t("updates.downloading", { version: status.version ?? "", progress: progress === null ? "…" : ` (${new Intl.NumberFormat(language).format(progress)}%)` })
    : status.phase === "disabled" ? t("updates.disabled")
    : status.phase === "error" ? t("updates.failed")
    : t("updates.automatic");
  return <aside className="notice update-notice" aria-label={t("updates.title")}>
    <p role="status">{message}</p>
    {ready && audioBusy ? <p>{t("updates.stopAudio")}</p> : null}
    {automatic ? <p>{t(pluralMessageId(language, seconds, { one: "updates.countdownOne", other: "updates.countdown" }), { seconds })}</p> : null}
    {updates.error || status.error ? <div role="alert"><p>{language === "en" ? updates.error ?? status.error : t("updates.error")}</p>
      <ErrorDetails error={{ code: "update_failed", message: updates.error ?? status.error ?? "", details: null, suggestedAction: null }} /></div> : null}
    {ready ? <button disabled={audioBusy || updates.installing} onClick={() => void updates.install()}>{t("updates.install")}</button>
      : helpOpen && (status.phase === "idle" || status.phase === "error") ? <button onClick={() => void updates.check()}>{t("updates.check")}</button> : null}
    {automatic ? <button onClick={() => setDeferredVersion(status.version)}>{t("updates.later")}</button> : null}
  </aside>;
}
