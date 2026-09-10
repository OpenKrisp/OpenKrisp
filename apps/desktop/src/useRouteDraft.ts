import { useEffect, useRef, useState } from "react";
import type { RouteSnapshot } from "./api";
import { defaultDraft, draftFromRoute, type RouteDraft, type RouteKind } from "./drafts";

export function useRouteDraft(kind: RouteKind, route: RouteSnapshot | undefined) {
  const [draft, setDraft] = useState(() => defaultDraft(kind));
  const restored = useRef<{ id: string; draft: RouteDraft } | null>(null);
  useEffect(() => {
    if (!route) return;
    const next = draftFromRoute(route) ?? (kind === "incoming" && "applicationStream" in route.config.source
      ? { ...defaultDraft("incoming"), mode: route.config.processingMode,
        denoise: { ...route.config.denoise }, targetLatencyMs: route.config.targetLatencyMs } : null);
    if (next?.kind === kind) {
      const previous = restored.current;
      restored.current = { id: route.config.id, draft: next };
      if (!previous || previous.id !== route.config.id) setDraft(next);
      else {
        // Only authoritative changes replace a field. Metrics and a bypass change must not
        // erase unrelated unsaved denoise edits or an idle user's loaded profile.
        const changes = Object.fromEntries(Object.entries(next).filter(([key, value]) =>
          JSON.stringify(value) !== JSON.stringify(previous.draft[key as keyof RouteDraft])));
        if (Object.keys(changes).length > 0) setDraft((draft) => ({ ...draft, ...changes }));
      }
    }
  }, [route, kind]);
  return [draft, setDraft] as const;
}
