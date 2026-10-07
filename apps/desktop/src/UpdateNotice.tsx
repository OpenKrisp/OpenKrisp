import { useEffect, useState } from "react";
import type { useUpdates } from "./useUpdates";

export function UpdateNotice({ updates, helpOpen, audioBusy }: {
  updates: ReturnType<typeof useUpdates>; helpOpen: boolean; audioBusy: boolean;
}) {
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
  const message = updates.installing ? "Installing update. OpenKrisp will restart…"
    : ready ? `OpenKrisp ${status.version} is ready to install.`
    : status.phase === "checking" ? "Checking for updates…"
    : status.phase === "downloading" ? `Downloading OpenKrisp ${status.version}${progress === null ? "…" : ` (${progress}%)`}`
    : status.phase === "disabled" ? "Automatic updates are not enabled in this build."
    : status.phase === "error" ? "Could not check for updates. We’ll try again automatically."
    : "OpenKrisp checks for updates automatically.";
  return <aside className="notice update-notice" aria-label="Application updates">
    <p role="status">{message}</p>
    {ready && audioBusy ? <p>Turn off both audio routes before installing.</p> : null}
    {automatic ? <p>Updating and restarting in {seconds} seconds.</p> : null}
    {updates.error || status.error ? <p role="alert">{updates.error ?? status.error}</p> : null}
    {ready ? <button disabled={audioBusy || updates.installing} onClick={() => void updates.install()}>Install and restart</button>
      : helpOpen && (status.phase === "idle" || status.phase === "error") ? <button onClick={() => void updates.check()}>Check for updates</button> : null}
    {automatic ? <button onClick={() => setDeferredVersion(status.version)}>Later</button> : null}
  </aside>;
}
