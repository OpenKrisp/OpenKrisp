import { useEffect, useRef, useState } from "react";
import type { RoutePreferences, RouteSnapshot } from "./api";
import { defaultDraft, draftFromRoute, type RouteDraft, type RouteKind } from "./drafts";
import { routeIsActive } from "./routing";
import { preferenceChanges } from "./preferences";

export function useRouteDraft(kind: RouteKind, route: RouteSnapshot | undefined, initial: RoutePreferences | undefined,
  onEdit: (previous: RouteDraft, next: RouteDraft) => void) {
  const [draft, setDraft] = useState(() => defaultDraft(kind));
  const current = useRef(draft);
  const hydrated = useRef(false);
  const restored = useRef<{ id: string; draft: RouteDraft } | null>(null);
  const preferredMode = useRef(initial?.mode);
  const wasActive = useRef(false);

  function replace(next: RouteDraft) { current.current = next; setDraft(next); }

  useEffect(() => {
    if (!initial) return;
    const next = route && (draftFromRoute(route) ?? (kind === "incoming" && "applicationStream" in route.config.source
      ? { ...defaultDraft("incoming"), mode: route.config.processingMode,
        denoise: { ...route.config.denoise }, targetLatencyMs: route.config.targetLatencyMs } : null));
    const active = routeIsActive(route);
    if (!hydrated.current) {
      hydrated.current = true;
      preferredMode.current = initial.mode;
      replace(active && next ? next : { ...initial, kind });
    } else if (active && next && route) {
      const previous = restored.current;
      if (!previous || previous.id !== route.config.id) replace(next);
      else {
        // Preserve dirty fields when an older Apply confirmation or a metric refresh arrives.
        const changes = preferenceChanges(previous.draft, next, current.current);
        if (Object.keys(changes).length > 0) replace({ ...current.current, ...changes });
      }
    } else if (wasActive.current && !active && preferredMode.current) {
      // Runtime bypass is temporary; the next Start uses the user's saved processing choice.
      replace({ ...current.current, mode: preferredMode.current });
    }
    if (next && route) restored.current = { id: route.config.id, draft: next };
    wasActive.current = active;
  }, [route, kind, initial]);

  function edit(next: RouteDraft) {
    const previous = current.current;
    if (!hydrated.current) return;
    if (next.mode !== previous.mode) preferredMode.current = next.mode;
    replace(next);
    onEdit(previous, next);
  }

  return [draft, edit] as const;
}
