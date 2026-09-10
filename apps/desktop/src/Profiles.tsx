import { useEffect, useRef, useState } from "react";
import { api, type AudioProfile, type EngineSnapshot, type ProfileCollection } from "./api";
import { draftFromProfile, draftSettingsError, profileFromDraft, profileNameError, type RouteDraft, type RouteKind } from "./drafts";
import { normalizeApiError } from "./state";

interface Target { draft: RouteDraft; locked: boolean }
interface Props {
  snapshot: EngineSnapshot;
  connected: boolean;
  targets: Record<RouteKind, Target>;
  onLoad: (draft: RouteDraft) => void;
  onBusy: (busy: boolean) => void;
}

export function Profiles({ snapshot, connected, targets, onLoad, onBusy }: Props) {
  const [collection, setCollection] = useState<ProfileCollection | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [name, setName] = useState("");
  const [kind, setKind] = useState<RouteKind>("microphone");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const busyRef = useRef(false);
  const readVersion = useRef(0);
  const current = useRef({ targets, snapshot, connected });
  current.current = { targets, snapshot, connected };
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    const version = ++readVersion.current;
    void api.listProfiles().then((next) => {
      if (cancelled || version !== readVersion.current) return;
      setCollection(next);
      setSelectedId(next.selectedProfileId);
      setName(next.profiles.find((profile) => profile.id === next.selectedProfileId)?.name ?? "");
    }).catch((failure) => { if (!cancelled && version === readVersion.current) setError(normalizeApiError(failure).message); });
    return () => { cancelled = true; mounted.current = false; };
  }, []);

  const selected = collection?.profiles.find((profile) => profile.id === selectedId);
  const selectedKind = selected?.sourceKind === "system_loopback" ? "incoming" : "microphone";
  const locked = busy || !connected || snapshot.shuttingDown;
  const draft = targets[kind].draft;
  const saveError = profileNameError(name) ?? draftSettingsError(draft) ??
    (!draft.inputId || !draft.outputId ? "Choose both endpoints in the route before saving a profile." : null);

  async function refresh(preferredId?: string) {
    const version = ++readVersion.current;
    const next = await api.listProfiles();
    if (!mounted.current || version !== readVersion.current) return;
    const id = next.profiles.some((profile) => profile.id === preferredId) ? preferredId! : next.selectedProfileId;
    setCollection(next); setSelectedId(id);
    setName(next.profiles.find((profile) => profile.id === id)?.name ?? "");
    setDeleteId(null);
  }

  async function run(action: () => Promise<void>) {
    if (busyRef.current || !connected || snapshot.shuttingDown) return;
    busyRef.current = true; setBusy(true); onBusy(true); setError(null); setMessage(null);
    try { await action(); }
    catch (failure) { if (mounted.current) setError(normalizeApiError(failure).message); }
    finally {
      busyRef.current = false;
      if (mounted.current) { setBusy(false); onBusy(false); }
    }
  }

  async function load(profile: AudioProfile) {
    const target = profile.sourceKind === "physical_input" ? "microphone" : "incoming";
    if (current.current.targets[target].locked) return;
    // A corrupt/newer file is never replaced by merely loading the fallback into the controls.
    if (!collection?.loadWarning) await api.selectProfile(profile.id);
    if (!mounted.current) return;
    if (current.current.targets[target].locked || !current.current.connected || current.current.snapshot.shuttingDown) {
      setMessage("The route changed while loading. Stop it before loading this profile again.");
      return;
    }
    onLoad(draftFromProfile(profile, current.current.snapshot));
    setMessage(`Loaded ${profile.name} into the ${target} controls. Audio has not been started.`);
    if (!collection?.loadWarning) await refresh(profile.id);
  }

  async function save(replace: boolean) {
    if (saveError || !collection) return;
    const id = replace && selected ? selected.id : `profile-${crypto.randomUUID()}`;
    await api.saveProfile(profileFromDraft(draft, id, name));
    // Save and selection are separate persisted operations; do not silently load/start a route.
    await refresh(id);
    if (mounted.current) setMessage("Profile saved. Load it when you want to use these settings.");
  }

  return <section className="panel" aria-labelledby="profiles-title">
    <div className="panel-heading"><h2 id="profiles-title">Profiles</h2>
      <button disabled={locked} onClick={() => void run(() => refresh(selectedId))}>Refresh profiles</button>
    </div>
    <p>Save microphone or incoming settings. Loading fills the stopped route's controls; choose Start when you are ready.</p>
    {collection?.loadWarning ? <p role="alert">{collection.loadWarning.message} Saving will replace the unreadable saved configuration.</p> : null}
    {collection ? <>
      <fieldset disabled={locked}>
        <label>Saved profile<select value={selectedId} onChange={(event) => {
          const profile = collection.profiles.find((item) => item.id === event.target.value);
          setSelectedId(event.target.value); setName(profile?.name ?? ""); setDeleteId(null); setMessage(null);
        }}>
          {collection.profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.name} · {profile.sourceKind === "physical_input" ? "microphone" : "incoming"}</option>)}
        </select></label>
      </fieldset>
      <div className="actions">
        <button disabled={locked || !selected || targets[selectedKind].locked} onClick={() => selected && void run(() => load(selected))}>Load profile</button>
        <button disabled={locked || !selected || Boolean(collection.loadWarning)} onClick={() => setDeleteId(selectedId)}>Delete profile</button>
      </div>
      {selected && targets[selectedKind].locked ? <p className="hint">Stop the {selectedKind} route before loading this profile.</p> : null}
      {deleteId ? <div className="confirmation" role="group" aria-label="Confirm profile deletion">
        <p>Delete {collection.profiles.find((profile) => profile.id === deleteId)?.name}? Active audio is unaffected.</p>
        <div className="actions"><button disabled={locked} onClick={() => void run(async () => {
          await api.deleteProfile(deleteId); await refresh();
          if (mounted.current) setMessage("Profile deleted.");
        })}>Confirm delete</button><button disabled={locked} onClick={() => setDeleteId(null)}>Cancel deletion</button></div>
      </div> : null}
      <fieldset disabled={locked} className="profile-save">
        <legend>Save route settings</legend>
        <label>Settings to save<select value={kind} onChange={(event) => setKind(event.target.value as RouteKind)}>
          <option value="microphone">Microphone</option><option value="incoming">Incoming audio</option>
        </select></label>
        <label>Profile name<input value={name} maxLength={128} onChange={(event) => setName(event.target.value)} /></label>
      </fieldset>
      {saveError ? <p className="hint">{saveError}</p> : null}
      <div className="actions">
        <button disabled={locked || saveError !== null} onClick={() => void run(() => save(false))}>
          {collection.loadWarning ? "Replace unreadable profiles and save" : "Save as new profile"}
        </button>
        <button disabled={locked || !selected || saveError !== null || Boolean(collection.loadWarning)} onClick={() => void run(() => save(true))}>Update selected profile</button>
      </div>
      <p className="hint">Saving uses the values shown in the route controls, including edits not yet applied to running audio.</p>
    </> : <p>Profiles have not loaded yet. Use Refresh profiles to retry.</p>}
    {busy ? <p role="status">Updating profiles…</p> : null}
    {error ? <p role="alert">{error}</p> : null}
    {message ? <p role="status">{message}</p> : null}
  </section>;
}
