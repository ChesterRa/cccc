import { useEffect, type RefObject } from "react";
import { markVoiceMessagesViewed } from "../../services/api/codexVoice";

// Visibility is deliberately stricter than Chat's read-through/Mail cursor.
export function fullyVisibleMessage(row: HTMLElement): boolean {
  if (document.visibilityState !== "visible" || !document.hasFocus()) return false;
  if (row.querySelector("details:not([open])")) return false;
  const rect = row.getBoundingClientRect();
  if (
    rect.width <= 0 ||
    rect.height <= 0 ||
    rect.top < 0 ||
    rect.left < 0 ||
    rect.bottom > window.innerHeight ||
    rect.right > window.innerWidth
  )
    return false;
  // Hit testing accounts for scroll clipping, dialogs and the Voice overlay.
  return [
    [rect.left + 2, rect.top + 2],
    [rect.right - 2, rect.top + 2],
    [rect.left + 2, rect.bottom - 2],
    [rect.right - 2, rect.bottom - 2],
    [(rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2],
  ].every(([x, y]) => {
    const hit = document.elementFromPoint(x, y);
    return hit !== null && row.contains(hit);
  });
}

export function useVoiceViewedMessages(
  root: RefObject<HTMLDivElement | null>,
  groupId: string,
  enabled: boolean,
) {
  useEffect(() => {
    if (!enabled || !groupId || typeof document.elementFromPoint !== "function") return;
    const since = new Map<string, number>();
    const observed = new Set<string>();
    let active = true;
    let posting = false;
    const reset = () => since.clear();
    const timer = window.setInterval(() => {
      if (document.visibilityState !== "visible" || !document.hasFocus()) {
        reset();
        return;
      }
      const now = performance.now();
      const visible = new Set<string>();
      const ready: string[] = [];
      for (const row of root.current?.querySelectorAll<HTMLElement>(
        '[data-voice-viewable="true"]',
      ) ?? []) {
        const id = row.dataset.messageId;
        if (!id || observed.has(id) || !fullyVisibleMessage(row)) continue;
        visible.add(id);
        const start = since.get(id);
        if (start === undefined) since.set(id, now);
        else if (now - start >= 1500 && ready.length < 128) ready.push(id);
      }
      for (const id of since.keys()) if (!visible.has(id)) since.delete(id);
      if (posting || ready.length === 0) return;
      posting = true;
      void markVoiceMessagesViewed(ready.map((event_id) => ({ group_id: groupId, event_id })))
        .then((response) => {
          if (active && response.ok) for (const id of ready) observed.add(id);
          // Revoked/scoped access must not keep submitting global observations.
          if (
            !response.ok &&
            ["forbidden", "unauthorized", "auth_required"].includes(response.error.code)
          )
            window.clearInterval(timer);
        })
        .catch(() => {
          /* A failed observation is not treated as viewed. */
        })
        .finally(() => {
          posting = false;
        });
    }, 500);
    window.addEventListener("blur", reset);
    document.addEventListener("visibilitychange", reset);
    root.current?.addEventListener("scroll", reset, true);
    const element = root.current;
    return () => {
      active = false;
      window.clearInterval(timer);
      window.removeEventListener("blur", reset);
      document.removeEventListener("visibilitychange", reset);
      element?.removeEventListener("scroll", reset, true);
    };
  }, [root, groupId, enabled]);
}
