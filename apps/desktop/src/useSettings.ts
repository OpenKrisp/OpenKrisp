import { useCallback, useEffect, useRef, useState } from "react";
import { api, type DesktopSettings, type EngineSnapshot, type SettingsSnapshot } from "./api";
import { draftSettingsError, type RouteDraft } from "./drafts";
import { initialSettings, preferenceChanges, sameSettings } from "./preferences";
import { normalizeApiError } from "./state";

/** Preferences are changed only by initialization or explicit control edits, never by audio events. */
export function useSettings(snapshot: EngineSnapshot | null) {
  const [initial, setInitial] = useState<DesktopSettings | null>(null);
  const [status, setStatus] = useState<SettingsSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const mounted = useRef(false);
  const preferences = useRef<DesktopSettings | null>(null);
  const pendingEdits = useRef<Partial<DesktopSettings>>({});
  const revision = useRef(0);
  const latestStatus = useRef<SettingsSnapshot | null>(null);
  const readVersion = useRef(0);
  const loaded = useRef<SettingsSnapshot | null>(null);
  const [loadTick, setLoadTick] = useState(0);
  const replacementAllowed = useRef(false);

  const observe = useCallback((next: SettingsSnapshot) => {
    if (!mounted.current || next.revision < revision.current ||
      (latestStatus.current && next.revision === latestStatus.current.revision && next.savedRevision < latestStatus.current.savedRevision)) return;
    latestStatus.current = next;
    setStatus(next);
  }, []);

  const load = useCallback(async () => {
    const version = ++readVersion.current;
    setLoading(true); setError(null);
    try {
      const next = await api.getSettings();
      if (!mounted.current || version !== readVersion.current) return;
      loaded.current = next;
      revision.current = next.revision;
      observe(next);
      setLoadTick((value) => value + 1);
    } catch (failure) {
      if (mounted.current && version === readVersion.current) setError(normalizeApiError(failure).message);
    } finally {
      if (mounted.current && version === readVersion.current) setLoading(false);
    }
  }, [observe]);

  useEffect(() => {
    mounted.current = true;
    void load();
    return () => { mounted.current = false; ++readVersion.current; };
  }, [load]);

  const save = useCallback((settings: DesktopSettings, replaceUnreadable = false) => {
    preferences.current = settings;
    if (latestStatus.current?.loadWarning && !replaceUnreadable && !replacementAllowed.current) return;
    if (replaceUnreadable) replacementAllowed.current = true;
    const nextRevision = ++revision.current;
    setError(null);
    // Invoke immediately. Debouncing belongs to Rust so Quit can drain accepted changes.
    setStatus((previous) => previous ? { ...previous, revision: nextRevision, writeError: null } : previous);
    void api.saveSettings({ settings, revision: nextRevision, replaceUnreadable: replacementAllowed.current }).then((next) => {
      if (!mounted.current) return;
      if (nextRevision === revision.current) { setError(null); observe(next); }
    }).catch((failure) => {
      if (mounted.current && nextRevision === revision.current) setError(normalizeApiError(failure).message);
    });
  }, [observe]);

  useEffect(() => {
    if (!snapshot || !loaded.current || preferences.current) return;
    const next = initialSettings(loaded.current.settings, snapshot);
    preferences.current = next;
    setInitial(next);
    if (!sameSettings(next, loaded.current.settings)) save(next);
  }, [snapshot, loadTick, save]);

  // Poll only while accepted edits are waiting for disk. These reads never refill the controls.
  useEffect(() => {
    if (!status || status.savedRevision >= status.revision || status.writeError || error) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await api.getSettings();
        if (cancelled) return;
        observe(next);
        if (next.revision >= revision.current && (next.savedRevision >= revision.current || next.writeError)) return;
      } catch (failure) {
        if (!cancelled) setError(normalizeApiError(failure).message);
        return;
      }
      timer = setTimeout(() => void poll(), 250);
    };
    timer = setTimeout(() => void poll(), 250);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [status?.revision, status?.savedRevision, status?.writeError, error, observe]);

  const edit = useCallback((previous: RouteDraft, next: RouteDraft) => {
    if (!preferences.current) return;
    const changes = preferenceChanges(previous, next);
    const route = { ...(pendingEdits.current[next.kind] ?? preferences.current[next.kind]), ...changes };
    pendingEdits.current[next.kind] = route;
    if (draftSettingsError({ ...route, kind: next.kind })) return;
    const settings = { ...preferences.current, [next.kind]: route };
    if (!sameSettings(settings, preferences.current)) save(settings);
  }, [save]);

  const retry = useCallback(async (replaceUnreadable = false) => {
    if (!preferences.current) { await load(); return; }
    // A read also reconciles revisions after a rejected/reordered IPC request. Never apply its values.
    const atRevision = revision.current;
    try {
      const next = await api.getSettings();
      if (!mounted.current || revision.current !== atRevision) return;
      revision.current = Math.max(revision.current, next.revision);
      save(preferences.current, replaceUnreadable);
    } catch (failure) { if (mounted.current) setError(normalizeApiError(failure).message); }
  }, [load, save]);

  return { initial, edit, retry, loading, status, error,
    saving: Boolean(status && status.revision > status.savedRevision && !status.writeError && !error),
    // Initial controls wait for both the settings read and device inventory; Stop never waits.
    ready: initial !== null };
}
