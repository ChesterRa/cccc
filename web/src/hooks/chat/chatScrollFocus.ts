import type { ChatScrollSnapshot } from "../../stores/useUIStore";
import type { LedgerEvent } from "../../types";

function confirmedId(event: LedgerEvent): string {
  const data = event.data as { _optimistic?: unknown } | undefined;
  return event.id && !data?._optimistic ? String(event.id) : "";
}

/**
 * The message a newly opened filter shows in place of the one being read: that message
 * when the view has it, else its nearest neighbour in the loaded conversation that the
 * view also has, the older one on a tie. Filters are often disjoint, so neighbours come
 * from the whole conversation rather than the filter just left. Empty when the message is
 * not in the loaded conversation or the view shares nothing with it; the view then opens
 * at its own default and the saved place is kept.
 */
export function pickScrollFocusTarget(input: {
  focusId: string;
  conversation: readonly LedgerEvent[];
  view: readonly LedgerEvent[];
}): string {
  const focusId = input.focusId.trim();
  if (!focusId) return "";
  const inView = new Set(input.view.map(confirmedId).filter(Boolean));
  if (inView.has(focusId)) return focusId;
  const order = input.conversation.map(confirmedId).filter(Boolean);
  const at = order.indexOf(focusId);
  if (at < 0) return "";
  for (let step = 1; step < order.length; step += 1) {
    const older = order[at - step];
    if (older && inView.has(older)) return older;
    const newer = order[at + step];
    if (newer && inView.has(newer)) return newer;
    if (at - step < 0 && at + step >= order.length) break;
  }
  return "";
}

export type InitialChatScroll = {
  anchorId?: string;
  anchorOffsetPx?: number;
  scrollOffsetPx?: number;
  /** The anchor stands in for a message the reader did not see here; mark it briefly. */
  highlight?: boolean;
};

/**
 * Where a newly opened chat view starts. A place saved in this view comes back exactly; one at
 * its bottom opens at the bottom. From another filter, the first message that was on screen
 * there (the one being read first) and is shown here keeps its height, so what the reader was
 * looking at stays put. When none is, the nearest neighbour of the message being read opens at
 * the top, marked, since a place inside one message means nothing for another. A place at the
 * bottom of another view hands on only a message the reader touched there. `exactOnly` views
 * (Waiting for you, a to-do list that opens at its newest request) take no neighbour.
 */
export function resolveInitialChatScroll(input: {
  /** A saved place worth restoring (see shouldRestoreScrollSnapshot), else null. */
  snapshot: ChatScrollSnapshot | null;
  viewKey: string;
  conversation: readonly LedgerEvent[];
  view: readonly LedgerEvent[];
  exactOnly: boolean;
}): InitialChatScroll {
  const { snapshot } = input;
  if (!snapshot) return {};
  if (!snapshot.viewKey || snapshot.viewKey === input.viewKey) {
    if (snapshot.mode !== "detached") return {};
    const scrollTop = Number(snapshot.scrollTop);
    return {
      anchorId: snapshot.anchorId,
      anchorOffsetPx: Number(snapshot.offsetPx || 0),
      scrollOffsetPx: Number.isFinite(scrollTop) ? Math.max(0, scrollTop) : undefined,
    };
  }
  const focusId = String(snapshot.focusId || snapshot.anchorId || "").trim();
  const onScreen = snapshot.visible?.length
    ? snapshot.visible
    : focusId
      ? [{ id: focusId, offsetPx: Number(snapshot.focusOffsetPx ?? snapshot.offsetPx ?? 0) }]
      : [];
  const inView = new Set(input.view.map(confirmedId).filter(Boolean));
  const kept = onScreen.find((message) => inView.has(String(message.id || "").trim()));
  if (kept) return { anchorId: kept.id, anchorOffsetPx: Number(kept.offsetPx || 0) };
  if (snapshot.mode !== "detached" || input.exactOnly) return {};
  const target = pickScrollFocusTarget({
    focusId,
    conversation: input.conversation,
    view: input.view,
  });
  return target ? { anchorId: target, highlight: true } : {};
}
