import { isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { monitorUpdates, updateApi, updateError, type UpdateStatus } from "./updates";

export function useUpdates() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const monitor = useRef<ReturnType<typeof monitorUpdates> | null>(null);
  const installPending = useRef(false);
  useEffect(() => {
    if (!isTauri()) return;
    const checks = monitorUpdates(updateApi, setStatus);
    monitor.current = checks;
    return () => { checks.stop(); monitor.current = null; };
  }, []);

  const install = useCallback(async () => {
    if (installPending.current) return;
    installPending.current = true; setInstalling(true); setError(null);
    try { await updateApi.install(); }
    catch (failure) { setError(updateError(failure)); }
    finally {
      await monitor.current?.refresh();
      installPending.current = false; setInstalling(false);
    }
  }, []);
  return { status, installing: installing || status?.phase === "installing", error,
    check: () => monitor.current?.check(), install };
}
