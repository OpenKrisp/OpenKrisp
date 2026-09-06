import { useEffect, useReducer, useState } from "react";

import { api, subscribeToEngineEvents, type ProfileCollection } from "./api";
import { initialDesktopState, normalizeApiError, reduceDesktopState } from "./state";

export function App() {
  const [state, dispatch] = useReducer(reduceDesktopState, initialDesktopState);
  const [profiles, setProfiles] = useState<ProfileCollection | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unsubscribe: (() => void) | undefined;
    const reconcile = async () => {
      try {
        const removeListeners = await subscribeToEngineEvents((event) => {
          if (event.type === "devices_changed") {
            void api
              .getEngineSnapshot()
              .then((next) => {
                if (!cancelled) dispatch({ type: "reconcile", snapshot: next });
              })
              .catch((error: unknown) => {
                if (!cancelled) {
                  dispatch({ type: "connection_failed", error: normalizeApiError(error) });
                }
              });
          } else if (!cancelled) {
            dispatch({ type: "event", event });
          }
        });
        if (cancelled) {
          removeListeners();
          return;
        }
        unsubscribe = removeListeners;

        const [snapshot, profileSnapshot] = await Promise.all([
          api.getEngineSnapshot(),
          api.listProfiles(),
        ]);
        if (!cancelled) {
          dispatch({ type: "reconcile", snapshot });
          setProfiles(profileSnapshot);
        }
      } catch (error) {
        if (!cancelled) {
          dispatch({ type: "connection_failed", error: normalizeApiError(error) });
        }
      }
    };
    void reconcile();
    return () => {
      cancelled = true;
      unsubscribe?.();
    };
  }, []);

  const route = state.snapshot?.routes[0];
  return (
    <main>
      <div className="eyebrow">OPENKRISP / DESKTOP MVP</div>
      <section className="card">
        <div className={`status-dot ${state.connected ? "online" : ""}`} aria-hidden="true" />
        <div>
          <h1>{state.connected ? "Audio engine connected" : "Connecting to audio engine"}</h1>
          <p>
            Phase 4 control-plane bridge is active. Device controls and live diagnostics arrive in
            the next UI slice.
          </p>
        </div>
      </section>
      <dl className="facts">
        <div>
          <dt>Host</dt>
          <dd>{state.snapshot?.inventory.host || "—"}</dd>
        </div>
        <div>
          <dt>Devices</dt>
          <dd>{state.snapshot?.inventory.devices.length ?? "—"}</dd>
        </div>
        <div>
          <dt>Route</dt>
          <dd>{route?.state ?? "stopped"}</dd>
        </div>
        <div>
          <dt>Profile</dt>
          <dd>{profiles?.selectedProfileId ?? "—"}</dd>
        </div>
      </dl>
      {state.fault ? (
        <aside role="alert">
          <strong>{state.fault.message}</strong>
          {state.fault.suggestedAction ? <span>{state.fault.suggestedAction}</span> : null}
        </aside>
      ) : null}
    </main>
  );
}
